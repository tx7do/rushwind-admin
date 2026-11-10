//! AiChatService — 对话闭环：用户消息落库 →（可选 RAG 检索注入）→ LLM 补全
//! → 助手消息落库 → 用量挂账 → 会话 last_message_at 前移。
//! knowledge_base_id 携带时注入检索上下文（RAG 通道随 3.10c 接 pgvector，
//! 现阶段知识库上下文为空注入——回答照常生成，语义面不缺位）。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::data::Viewer;
use crate::state::{db_err, not_found, operator_of, status_error, AppState, StatusError};
use proto::proto::ai::service::v1::{AiConversation, AiMessage, ChatRequest, ChatResponse};

fn conv_proto(r: crate::data::sys_ai_conversations::Model) -> AiConversation {
    AiConversation {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        title: r.title,
        provider_id: r.provider_id.map(|v| v as u64),
        user_id: Some(r.user_id as u64),
        last_message_at: r.last_message_at.and_then(crate::state::naive_to_ts),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn msg_proto(r: crate::data::sys_ai_messages::Model) -> AiMessage {
    AiMessage {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        conversation_id: Some(r.conversation_id as u64),
        role: r.role.as_deref().map(|s| match s {
            "ASSISTANT" => 1,
            "SYSTEM" => 2,
            _ => 0,
        }),
        content: r.content,
        model_name: r.model_name,
        prompt_tokens: r.prompt_tokens,
        completion_tokens: r.completion_tokens,
        duration_ms: r.duration_ms,
        error_message: r.error_message,
        user_id: Some(r.user_id as u64),
        created_by: None,
        updated_by: None,
        deleted_by: None,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: None,
        deleted_at: None,
    }
}

pub struct AiChatService {
    pub state: Arc<AppState>,
}

impl AiChatService {
    /// 会话内最近 N 轮作为上下文（与上游 chatContextMaxMessages 的窗口语义同形）。
    async fn context_messages(
        &self,
        conversation_id: i64,
    ) -> Result<Vec<super::llm::ChatMessage>, StatusError> {
        use sea_orm::QueryOrder as _;
        use sea_orm::QuerySelect as _;
        let rows = crate::data::sys_ai_messages::Entity::find()
            .filter(crate::data::sys_ai_messages::Column::ConversationId.eq(conversation_id))
            .order_by_desc(crate::data::sys_ai_messages::Column::Id)
            .limit(20)
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows.into_iter().rev() {
            let role = match row.role.as_deref() {
                Some("ASSISTANT") => "assistant",
                Some("SYSTEM") => "system",
                _ => "user",
            };
            out.push(super::llm::ChatMessage {
                role,
                content: row.content.unwrap_or_default(),
            });
        }
        Ok(out)
    }

    async fn load_provider(
        &self,
        tenant_id: i64,
        provider_id: Option<i64>,
    ) -> Result<crate::data::sys_ai_providers::Model, StatusError> {
        let viewer = Viewer::system();
        let repo = crate::data::repos::AiProviderRepo::new(&self.state.db, viewer);
        let row = match provider_id {
            Some(id) => repo
                .find_scoped(id)
                .await?
                .ok_or_else(|| not_found("ai provider"))?,
            None => repo
                .find_default()
                .await?
                .ok_or_else(|| status_error("BAD_REQUEST", "no ai provider configured"))?,
        };
        if row.is_enabled != Some(true) {
            return Err(status_error("BAD_REQUEST", "ai provider is disabled"));
        }
        let _ = tenant_id;
        Ok(row)
    }
}

#[async_trait::async_trait]
impl proto::gen::services::AiChatServiceHandlers for AiChatService {
    async fn chat(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ChatRequest,
    ) -> Result<ChatResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let content = req
            .content
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string();
        if content.is_empty() {
            return Err(status_error("BAD_REQUEST", "content is required"));
        }

        // 会话解析：显式 id 优先；缺省时以内容前 20 字为题建会话。
        let conversation = if let Some(cid) = req.conversation_id.filter(|c| *c > 0) {
            crate::data::repos::AiConversationRepo::new(&self.state.db, Viewer::from_ctx(&ctx))
                .find_scoped(cid as i64)
                .await?
                .ok_or_else(|| not_found("ai conversation"))?
        } else {
            let title: String = content.chars().take(20).collect();
            crate::data::sys_ai_conversations::ActiveModel {
                tenant_id: Set(Some(payload.tenant_id)),
                title: Set(Some(title)),
                provider_id: Set(req.provider_id.map(|v| v as i64)),
                user_id: Set(payload.user_id),
                created_by: Set(Some(payload.user_id)),
                updated_by: Set(Some(payload.user_id)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?
        };

        // Provider 解析：载荷 > 会话绑定 > 默认。
        let provider_id = req
            .provider_id
            .map(|v| v as i64)
            .or(conversation.provider_id);
        let provider = self.load_provider(payload.tenant_id, provider_id).await?;

        // 用户消息落库。
        crate::data::sys_ai_messages::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            conversation_id: Set(conversation.id),
            role: Set(Some("USER".into())),
            content: Set(Some(content.clone())),
            model_name: Set(provider.model_name.clone()),
            user_id: Set(payload.user_id),
            created_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        // LLM 补全：系统提示词 + 上下文窗口 + 本轮用户消息。
        let mut messages = Vec::new();
        if let Some(sys) = provider.system_prompt.as_deref().filter(|s| !s.is_empty()) {
            messages.push(super::llm::ChatMessage {
                role: "system",
                content: sys.to_string(),
            });
        }
        messages.extend(self.context_messages(conversation.id).await?);
        let completion = super::llm::chat_completion(&provider, messages)
            .await
            .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;

        // 助手消息落库。
        let assistant = crate::data::sys_ai_messages::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            conversation_id: Set(conversation.id),
            role: Set(Some("ASSISTANT".into())),
            content: Set(Some(completion.content.clone())),
            model_name: Set(Some(completion.model.clone())),
            prompt_tokens: Set(Some(completion.prompt_tokens)),
            completion_tokens: Set(Some(completion.completion_tokens)),
            duration_ms: Set(Some(completion.duration_ms)),
            user_id: Set(payload.user_id),
            created_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        // 用量挂账（台账行；月汇总由 AiUsageLog 聚合）。
        crate::data::repos::AiUsageLogRepo::new(&self.state.db)
            .insert(crate::data::sys_ai_usage_logs::ActiveModel {
                tenant_id: Set(Some(payload.tenant_id)),
                provider_id: Set(Some(provider.id)),
                conversation_id: Set(Some(conversation.id)),
                user_id: Set(Some(payload.user_id)),
                model_name: Set(Some(completion.model.clone())),
                prompt_tokens: Set(Some(completion.prompt_tokens)),
                completion_tokens: Set(Some(completion.completion_tokens)),
                total_tokens: Set(Some(
                    completion.prompt_tokens + completion.completion_tokens,
                )),
                duration_ms: Set(Some(completion.duration_ms)),
                created_at: Set(Some(crate::data::now())),
                ..Default::default()
            })
            .await?;

        // 会话 last_message_at 前移。
        {
            let mut a: crate::data::sys_ai_conversations::ActiveModel = conversation.clone().into();
            a.last_message_at = Set(Some(crate::data::now()));
            a.updated_at = Set(Some(crate::data::now()));
            a.update(&self.state.db).await.map_err(db_err)?;
        }

        // 会话标题回读（新建时标题已定；更新过的 last_message_at 不回传，读面自取）。
        Ok(ChatResponse {
            conversation: Some(conv_proto(conversation)),
            message: Some(msg_proto(assistant)),
        })
    }
}
