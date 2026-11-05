//! AiContentService — 内容生成（场景 prompt 与上游逐字一致）+ 菜单语义
//! 搜索（sys_ai_search_index 索引表：重建 = 菜单树收集 → 批量 embedding →
//! 全量换表；检索 = 查询向量化 → pgvector 余弦）。
//!
//! 索引表不在 SeaORM 模型内（与 chunks 的向量面同口径）：建表在启动迁移
//! （m20251105_000010），读写走原生 SQL。

use std::sync::Arc;

use sea_orm::{ConnectionTrait, EntityTrait, Statement};

use crate::state::{operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{
    GenerateContentRequest, GenerateContentResponse, RebuildSearchIndexResponse,
    SemanticSearchItem, SemanticSearchRequest, SemanticSearchResponse,
};

/// 场景 → 系统提示词（与上游 aiContentScenePrompts 逐字一致；%s = 输出语言）。
fn scene_prompt(scene: i32, lang: &str) -> String {
    match scene {
        1 => "你是企业后台的内容撰写助手。根据用户给出的主题，为后台管理系统的实体（角色/菜单/字典项/文件等）撰写一段简明的描述文字。要求：专业、简洁、准确，不超过 100 字。用%s输出，只输出正文。",
        2 => "你是企业后台的公告撰写助手。根据用户给出的主题，撰写一则面向全体员工的通知公告。要求：包含标题行、正文（背景/内容/要求），语气正式但不生硬，总长度不超过 300 字。用%s输出，只输出正文。",
        3 => "你是企业后台的客服回复助手。根据用户给出的问题或场景，撰写一段专业的回复文案。要求：语气友好、有条理、针对性强，不超过 200 字。用%s输出，只输出正文。",
        _ => "你是企业后台的智能写作助手。根据用户的指令生成内容。要求：直接输出结果，不要解释你的思考过程。用%s输出。",
    }
    .replace("%s", lang)
}

/// 待索引菜单收集后的文档（真实菜单 id + 标题 + 路由）。
struct MenuIndexDoc {
    menu_id: u32,
    title: String,
    route: String,
}

/// 从菜单行收集可索引文档：跳过无 id / 无 path / 无标题的菜单；标题取
/// meta.title，为空回退 name（与全局搜索原始行为一致）。
fn collect_menu_index_docs(rows: Vec<crate::data::sys_menus::Model>) -> Vec<MenuIndexDoc> {
    rows.into_iter()
        .filter(|m| m.path.as_deref().map(|p| !p.is_empty()).unwrap_or(false))
        .map(|m| {
            let title = m
                .meta
                .as_ref()
                .and_then(|v| v.get("title"))
                .and_then(|t| t.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| m.name.clone());
            MenuIndexDoc {
                menu_id: m.id,
                title,
                route: m.path.unwrap_or_default(),
            }
        })
        .filter(|d| !d.title.is_empty())
        .collect()
}

pub struct AiContentService {
    pub state: Arc<AppState>,
}

impl AiContentService {
    /// 默认启用 provider（生成/索引/检索共用的取数锚）。
    async fn default_provider(&self) -> Result<crate::data::sys_ai_providers::Model, StatusError> {
        crate::data::repos::AiProviderRepo::new(&self.state.db, crate::data::Viewer::system())
            .find_default()
            .await?
            .ok_or_else(|| status_error("BAD_REQUEST", "no ai provider configured"))
    }

    /// 重建索引：TRUNCATE 后按批 embedding 写入（对齐响应 index 校验
    /// 由 llm.rs 的对位返回保证——数与文一一对应）。
    async fn rebuild_index_inner(&self, tenant_id: u32, user_id: u32) -> Result<u32, StatusError> {
        let menus = crate::data::sys_menus::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(crate::state::db_err)?;
        let docs = collect_menu_index_docs(menus);
        self.state
            .db
            .execute_unprepared("TRUNCATE sys_ai_search_index")
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", format!("truncate index: {e}")))?;
        if docs.is_empty() {
            return Ok(0);
        }
        let provider = self.default_provider().await?;
        // embedding 模型：索引面无独立配置，跟随 provider.model_name 的嵌入
        // 通道缺省（text-embedding-3-small 与上游同缺省）。
        let model = "text-embedding-3-small";
        let texts: Vec<String> = docs
            .iter()
            .map(|d| format!("{} {}", d.title, d.route))
            .collect();
        let started = std::time::Instant::now();
        let vectors = super::llm::embeddings(&provider, model, texts)
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;
        let duration_ms = started.elapsed().as_millis() as u32;
        let _ = crate::data::repos::AiUsageLogRepo::new(&self.state.db)
            .insert(crate::data::sys_ai_usage_logs::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                provider_id: Set(Some(provider.id)),
                conversation_id: Set(None),
                user_id: Set(Some(user_id)),
                model_name: Set(Some(model.into())),
                prompt_tokens: Set(Some(0)),
                completion_tokens: Set(Some(0)),
                total_tokens: Set(Some(0)),
                duration_ms: Set(Some(duration_ms)),
                created_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await;
        let mut inserted = 0u32;
        for (d, v) in docs.iter().zip(&vectors) {
            let vec = v
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "INSERT INTO sys_ai_search_index (item_type, item_id, title, route, embedding) \
                 VALUES ('menu', {}, '{}', '{}', '{}::vector')",
                d.menu_id,
                d.title.replace('\'', "''"),
                d.route.replace('\'', "''"),
                vec
            );
            self.state
                .db
                .execute_unprepared(&sql)
                .await
                .map_err(|e| status_error("INTERNAL_SERVER_ERROR", format!("index insert: {e}")))?;
            inserted += 1;
        }
        Ok(inserted)
    }
}

use sea_orm::Set;

#[async_trait::async_trait]
impl proto::gen::services::AiContentServiceHandlers for AiContentService {
    /// 内容生成：场景 prompt（上游逐字）+ 可选上下文注入，LLM 直调补全。
    async fn generate_content(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GenerateContentRequest,
    ) -> Result<GenerateContentResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        if req.topic.trim().is_empty() {
            return Err(status_error("BAD_REQUEST", "topic is required"));
        }
        let lang = req
            .lang
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("zh-CN");
        let provider = self.default_provider().await?;
        let mut messages = Vec::new();
        messages.push(super::llm::ChatMessage {
            role: "system",
            content: scene_prompt(req.scene, lang),
        });
        let mut user_text = req.topic.clone();
        if let Some(ctx_text) = req.context.as_deref().filter(|s| !s.is_empty()) {
            user_text.push_str("\n\n参考上下文：\n");
            user_text.push_str(ctx_text);
        }
        messages.push(super::llm::ChatMessage {
            role: "user",
            content: user_text,
        });
        let result = super::llm::chat_completion(&provider, messages)
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;
        if result.prompt_tokens + result.completion_tokens > 0 {
            let _ = crate::data::repos::AiUsageLogRepo::new(&self.state.db)
                .insert(crate::data::sys_ai_usage_logs::ActiveModel {
                    tenant_id: Set(Some(payload.tenant_id)),
                    provider_id: Set(Some(provider.id)),
                    conversation_id: Set(None),
                    user_id: Set(Some(payload.user_id)),
                    model_name: Set(Some(result.model.clone())),
                    prompt_tokens: Set(Some(result.prompt_tokens)),
                    completion_tokens: Set(Some(result.completion_tokens)),
                    total_tokens: Set(Some(result.prompt_tokens + result.completion_tokens)),
                    duration_ms: Set(Some(result.duration_ms)),
                    created_at: Set(Some(crate::data::now())),
                    ..Default::default()
                })
                .await;
        }
        Ok(GenerateContentResponse {
            content: result.content,
            total_tokens: result.prompt_tokens + result.completion_tokens,
        })
    }

    /// 菜单语义检索：查询向量化 → 索引表余弦检索（空索引 = 空结果，不报错）。
    async fn semantic_search(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: SemanticSearchRequest,
    ) -> Result<SemanticSearchResponse, StatusError> {
        let _payload = operator_of(&ctx)?;
        let query = req.query.trim();
        if query.is_empty() {
            return Err(status_error("BAD_REQUEST", "query is required"));
        }
        let limit = req.limit.unwrap_or(5).clamp(1, 20) as i64;
        // 索引空 → 语义检索面未建，返回空（前端回退关键词搜索）。
        let count = self
            .state
            .db
            .query_one_raw(Statement::from_string(
                self.state.db.get_database_backend(),
                "SELECT COUNT(*) AS cnt FROM sys_ai_search_index",
            ))
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        if count == 0 {
            return Ok(SemanticSearchResponse { items: Vec::new() });
        }
        let provider = self.default_provider().await?;
        let vectors =
            super::llm::embeddings(&provider, "text-embedding-3-small", vec![query.to_string()])
                .await
                .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;
        let vec = vec_literal_of(vectors.first());
        let rows = self
            .state
            .db
            .query_all_raw(Statement::from_string(
                self.state.db.get_database_backend(),
                format!(
                    "SELECT title, route, item_type, 1 - (embedding <=> '{vec}'::vector) AS score \
                     FROM sys_ai_search_index ORDER BY embedding <=> '{vec}'::vector LIMIT {limit}"
                ),
            ))
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", format!("semantic search: {e}")))?;
        Ok(SemanticSearchResponse {
            items: rows
                .iter()
                .filter_map(|r| {
                    Some(SemanticSearchItem {
                        title: r.try_get("", "title").ok()?,
                        route: r.try_get("", "route").ok()?,
                        item_type: r.try_get("", "item_type").ok()?,
                        score: r.try_get::<f32>("", "score").unwrap_or(0.0),
                    })
                })
                .collect(),
        })
    }

    /// 重建菜单语义索引（管理页触发；返回入索条数）。
    async fn rebuild_search_index(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<RebuildSearchIndexResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let indexed = self
            .rebuild_index_inner(payload.tenant_id, payload.user_id)
            .await?;
        Ok(RebuildSearchIndexResponse {
            indexed_count: indexed,
        })
    }
}

fn vec_literal_of(v: Option<&Vec<f32>>) -> String {
    let items: Vec<String> = v
        .map(|xs| xs.iter().map(|x| x.to_string()).collect())
        .unwrap_or_default();
    format!("[{}]", items.join(","))
}
