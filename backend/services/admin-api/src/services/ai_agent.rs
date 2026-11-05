//! AiAgentService — Agent 应用 CRUD + 图运行。图非空时必须能解析且过静态
//! 校验（存一张画布都画不出的图没有任何价值）；Run 先落 RUNNING 行再执行，
//! 终态 SUCCESS/FAILED 带产出/错误/轨迹/用量回填——上游 SettleResult 同语义。

use std::collections::BTreeMap;
use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::data::Viewer;
use crate::state::{
    db_err, not_found, operator_of, require_data, status_error, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{
    AiAgentApp, AiAgentRun, CreateAiAgentAppRequest, DeleteAiAgentAppRequest, GetAiAgentAppRequest,
    GetAiAgentRunRequest, ListAiAgentAppsResponse, ListAiAgentRunsResponse, RunAiAgentAppRequest,
    UpdateAiAgentAppRequest,
};
use proto::proto::pagination::PagingRequest;

use super::agent_graph;

fn app_proto(r: crate::data::sys_ai_agent_apps::Model) -> AiAgentApp {
    AiAgentApp {
        id: Some(r.id),
        tenant_id: r.tenant_id,
        name: Some(r.name),
        description: r.description,
        graph: r.graph,
        enabled: r.enabled,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

fn run_proto(r: crate::data::sys_ai_agent_runs::Model) -> AiAgentRun {
    AiAgentRun {
        id: Some(r.id),
        app_id: Some(r.app_id),
        status: Some(match r.status.as_deref() {
            Some("SUCCEEDED") => 2,
            Some("FAILED") => 3,
            _ => 1, // RUNNING（含 PENDING 等中间态）
        }),
        input: r.input,
        output: r.output,
        error: r.error,
        trace: r.trace,
        prompt_tokens: r.prompt_tokens,
        completion_tokens: r.completion_tokens,
        total_tokens: r.total_tokens,
        duration_ms: r.duration_ms,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(
    a: &mut crate::data::sys_ai_agent_apps::ActiveModel,
    data: &AiAgentApp,
) -> Result<(), StatusError> {
    if let Some(v) = &data.name {
        a.name = Set(v.clone());
    }
    if let Some(v) = &data.description {
        a.description = Set(Some(v.clone()));
    }
    if let Some(v) = &data.graph {
        // 图非空时必须能解析且过静态校验。
        if !v.trim().is_empty() {
            let g =
                agent_graph::parse_agent_graph(v).map_err(|e| status_error("BAD_REQUEST", e))?;
            agent_graph::validate_agent_graph(&g).map_err(|e| status_error("BAD_REQUEST", e))?;
        }
        a.graph = Set(Some(v.clone()));
    }
    if let Some(v) = data.enabled {
        a.enabled = Set(Some(v));
    }
    Ok(())
}

pub struct AiAgentService {
    pub state: Arc<AppState>,
}

impl AiAgentService {
    async fn app_scoped(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        id: u32,
    ) -> Result<crate::data::sys_ai_agent_apps::Model, StatusError> {
        let mut query = crate::data::sys_ai_agent_apps::Entity::find_by_id(id);
        if let Some(tid) = Viewer::from_ctx(ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_agent_apps::Column::TenantId.eq(tid));
        }
        query
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("ai agent app"))
    }

    async fn run_scoped(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        id: u32,
    ) -> Result<crate::data::sys_ai_agent_runs::Model, StatusError> {
        let mut query = crate::data::sys_ai_agent_runs::Entity::find_by_id(id);
        if let Some(tid) = Viewer::from_ctx(ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_agent_runs::Column::TenantId.eq(tid));
        }
        query
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("ai agent run"))
    }

    /// LLM 节点真实现：provider 解析（节点点名，留空取默认启用）→ OpenAI
    /// 兼容补全。逐节点落用量流水（conversation_id=0 的非对话调用，与 chat
    /// 共用同一台账口径）；记账失败不回滚答案——运行本身已成功，台账缺一行
    /// 可补，答案丢了不可补。上游的多轮工具循环（唯一内置工具
    /// get_current_time）以单轮直调等价落地。
    async fn run_llm_for_agent(
        &self,
        tenant_id: u32,
        user_id: u32,
        node: &agent_graph::AgentGraphNode,
        system: &str,
        user: &str,
    ) -> Result<(String, agent_graph::AgentUsage), String> {
        let provider = {
            let viewer = Viewer::system();
            let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, viewer);
            let pid = agent_graph::data_u32(&node.data, "providerId");
            let row = if pid > 0 {
                repo.find_scoped(pid)
                    .await
                    .map_err(|e| e.message.clone())?
                    .ok_or_else(|| format!("ai provider [{pid}] not found"))?
            } else {
                repo.find_default()
                    .await
                    .map_err(|e| e.message.clone())?
                    .ok_or_else(|| "no enabled default ai provider".to_string())?
            };
            if row.is_enabled != Some(true) {
                return Err(format!("ai provider [{}] is disabled", row.id));
            }
            row
        };
        // 节点可点名模型（留空用 provider 的）。
        let model_override = {
            let m = node
                .data
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            (!m.is_empty()).then(|| m.to_string())
        };
        let mut provider_for_call = provider.clone();
        if let Some(m) = model_override {
            provider_for_call.model_name = Some(m);
        }

        let mut messages = Vec::new();
        if !system.trim().is_empty() {
            messages.push(super::llm::ChatMessage {
                role: "system",
                content: system.to_string(),
            });
        }
        messages.push(super::llm::ChatMessage {
            role: "user",
            content: user.to_string(),
        });

        let result = super::llm::chat_completion(&provider_for_call, messages).await?;

        // 逐 LLM 节点记一条用量流水（非对话调用）。
        if result.prompt_tokens + result.completion_tokens > 0 {
            let _ = crate::data::repos::AiUsageLogRepo::new(&self.state.db)
                .insert(crate::data::sys_ai_usage_logs::ActiveModel {
                    tenant_id: Set(Some(tenant_id)),
                    provider_id: Set(Some(provider.id)),
                    conversation_id: Set(None),
                    user_id: Set(Some(user_id)),
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
        Ok((
            result.content,
            agent_graph::AgentUsage {
                prompt: result.prompt_tokens,
                completion: result.completion_tokens,
                total: result.prompt_tokens + result.completion_tokens,
            },
        ))
    }

    /// 知识检索节点真实现：复用 chat 同一条 RAG 链路（embed + 余弦检索 +
    /// embed 用量记账），命中片段按序拼接为上下文。RAG 通道随 knowledge
    /// 服务落地（3.10d）；此前 knowledge 节点如实报错"知识库未就绪"。
    async fn search_knowledge_for_agent(
        &self,
        tenant_id: u32,
        user_id: u32,
        node: &agent_graph::AgentGraphNode,
        query: &str,
    ) -> Result<(String, usize), String> {
        let base_id = agent_graph::data_u32(&node.data, "baseId");
        if base_id == 0 {
            return Err("knowledge node has no baseId configured".into());
        }
        let top_k = agent_graph::data_i64(&node.data, "topK", 4).max(1) as usize;
        let hits = super::ai_knowledge::search_knowledge_base(
            &self.state,
            tenant_id,
            user_id,
            base_id,
            query,
            top_k,
        )
        .await
        .map_err(|e| e.message.clone())?;
        let joined = hits.join("\n\n");
        Ok((joined, hits.len()))
    }
}

#[async_trait::async_trait]
impl proto::gen::services::AiAgentServiceHandlers for AiAgentService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiAgentAppsResponse, StatusError> {
        let mut query = crate::data::sys_ai_agent_apps::Entity::find();
        if let Some(tid) = Viewer::from_ctx(&ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_agent_apps::Column::TenantId.eq(tid));
        }
        let (rows, total) = crate::paging::fetch_paged(&self.state.db, query, &req).await?;
        Ok(ListAiAgentAppsResponse {
            items: rows.into_iter().map(app_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiAgentAppRequest,
    ) -> Result<AiAgentApp, StatusError> {
        let id = req.id;
        Ok(app_proto(self.app_scoped(&ctx, id).await?))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateAiAgentAppRequest,
    ) -> Result<AiAgentApp, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let mut a = crate::data::sys_ai_agent_apps::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data)?;
        if a.enabled.clone().unwrap().is_none() {
            a.enabled = Set(Some(true));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(app_proto(inserted))
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiAgentAppRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let row = self.app_scoped(&ctx, req.id).await?;
        let mut a: crate::data::sys_ai_agent_apps::ActiveModel = row.into();
        upsert_fields(&mut a, &data)?;
        crate::stamp_update!(a, payload.user_id);
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiAgentAppRequest,
    ) -> Result<Empty, StatusError> {
        let id = req.id;
        self.app_scoped(&ctx, id).await?;
        // 级联清运行记录（上游 edge cascade 同语义）。
        crate::data::sys_ai_agent_runs::Entity::delete_many()
            .filter(crate::data::sys_ai_agent_runs::Column::AppId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_ai_agent_apps::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    /// 图运行：先落 RUNNING 行 → 校验并执行 → 终态回填（output/error/trace/
    /// 用量/时长）。执行失败同样返回运行行（status=FAILED + error），与上游
    /// "失败也是一条可观测记录"语义一致。
    async fn run(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RunAiAgentAppRequest,
    ) -> Result<AiAgentRun, StatusError> {
        let payload = operator_of(&ctx)?;
        let app = self.app_scoped(&ctx, req.id).await?;
        if app.enabled != Some(true) {
            return Err(status_error("BAD_REQUEST", "ai agent app is disabled"));
        }
        let g = agent_graph::parse_agent_graph(app.graph.as_deref().unwrap_or(""))
            .map_err(|e| status_error("BAD_REQUEST", e))?;
        agent_graph::validate_agent_graph(&g).map_err(|e| status_error("BAD_REQUEST", e))?;

        let started = std::time::Instant::now();
        let input_json = serde_json::to_string(&req.input)
            .map_err(|_| status_error("BAD_REQUEST", "invalid input"))?;

        // RUNNING 行先落库（崩溃时留痕）。
        let run = crate::data::sys_ai_agent_runs::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            app_id: Set(app.id),
            status: Set(Some("RUNNING".into())),
            input: Set(Some(input_json)),
            user_id: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        let input: BTreeMap<String, String> = req.input.into_iter().collect();
        let tenant_id = payload.tenant_id;
        let user_id = payload.user_id;
        let deps = agent_graph::Deps {
            search_knowledge: Box::new({
                let state = Arc::clone(&self.state);
                move |node, query| {
                    // 引擎是同步内核；检索是异步外呼——这里按上界等待桥接
                    // （tokio 壳内单步阻塞等待是安全的：不在 async 上下文内）。
                    let state = Arc::clone(&state);
                    let node = node.clone();
                    let query = query.to_string();
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            AiAgentService::search_knowledge_for_agent(
                                &AiAgentService {
                                    state: Arc::clone(&state),
                                },
                                tenant_id,
                                user_id,
                                &node,
                                &query,
                            )
                            .await
                        })
                    })
                }
            }),
            run_llm: Box::new({
                let state = Arc::clone(&self.state);
                move |node, system, user| {
                    let node = node.clone();
                    let (system, user) = (system.to_string(), user.to_string());
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            AiAgentService::run_llm_for_agent(
                                &AiAgentService {
                                    state: Arc::clone(&state),
                                },
                                tenant_id,
                                user_id,
                                &node,
                                &system,
                                &user,
                            )
                            .await
                        })
                    })
                }
            }),
        };

        let execution = agent_graph::execute_agent_graph(&g, &input, &deps);
        let duration_ms = started.elapsed().as_millis() as u32;

        let mut a: crate::data::sys_ai_agent_runs::ActiveModel = run.into();
        match execution {
            Ok(exec) => {
                a.status = Set(Some("SUCCEEDED".into()));
                a.output = Set(Some(exec.output));
                a.error = Set(Some(String::new()));
                a.trace = Set(Some(serde_json::to_string(&exec.trace).unwrap_or_default()));
                a.prompt_tokens = Set(Some(exec.usage.prompt));
                a.completion_tokens = Set(Some(exec.usage.completion));
                a.total_tokens = Set(Some(exec.usage.total));
            }
            Err(err) => {
                a.status = Set(Some("FAILED".into()));
                a.error = Set(Some(err));
                a.output = Set(Some(String::new()));
                a.prompt_tokens = Set(Some(0));
                a.completion_tokens = Set(Some(0));
                a.total_tokens = Set(Some(0));
            }
        }
        a.duration_ms = Set(Some(duration_ms));
        a.updated_at = Set(Some(crate::data::now()));
        let settled = a.update(&self.state.db).await.map_err(db_err)?;
        Ok(run_proto(settled))
    }

    async fn list_runs(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiAgentRunsResponse, StatusError> {
        let mut query = crate::data::sys_ai_agent_runs::Entity::find();
        if let Some(tid) = Viewer::from_ctx(&ctx).tenant_scope() {
            query = query.filter(crate::data::sys_ai_agent_runs::Column::TenantId.eq(tid));
        }
        let (rows, total) = crate::paging::fetch_paged(&self.state.db, query, &req).await?;
        Ok(ListAiAgentRunsResponse {
            items: rows.into_iter().map(run_proto).collect(),
            total,
        })
    }

    async fn get_run(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiAgentRunRequest,
    ) -> Result<AiAgentRun, StatusError> {
        let id = req.id;
        Ok(run_proto(self.run_scoped(&ctx, id).await?))
    }
}
