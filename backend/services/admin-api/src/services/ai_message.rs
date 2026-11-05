//! AiMessageService — 会话消息台账（只读列表 + 删除）。

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::data::Viewer;
use crate::state::{db_err, not_found, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{AiMessage, DeleteAiMessageRequest, ListAiMessageResponse};
use proto::proto::pagination::PagingRequest;

fn msg_proto(r: crate::data::sys_ai_messages::Model) -> AiMessage {
    AiMessage {
        id: Some(r.id),
        tenant_id: r.tenant_id,
        conversation_id: Some(r.conversation_id),
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
        user_id: Some(r.user_id),
        created_by: None,
        updated_by: None,
        deleted_by: None,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: None,
        deleted_at: None,
    }
}

pub struct AiMessageService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::AiMessageServiceHandlers for AiMessageService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiMessageResponse, StatusError> {
        let repo = crate::data::repos::AiMessageRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiMessageResponse {
            items: rows.into_iter().map(msg_proto).collect(),
            total,
        })
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiMessageRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::delete_ai_message_request::QueryBy
        );
        let repo = crate::data::repos::AiMessageRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        repo.find_scoped(id)
            .await?
            .ok_or_else(|| not_found("ai message"))?;
        crate::data::sys_ai_messages::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
