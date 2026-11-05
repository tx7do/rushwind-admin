//! AiConversationService — 会话 CRUD（读面按操作者钉定，平台用户看全量）。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, Set};

use crate::data::Viewer;
use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{
    AiConversation, DeleteAiConversationRequest, GetAiConversationRequest,
    ListAiConversationResponse, UpdateAiConversationRequest,
};
use proto::proto::pagination::PagingRequest;

fn conv_proto(r: crate::data::sys_ai_conversations::Model) -> AiConversation {
    AiConversation {
        id: Some(r.id),
        tenant_id: r.tenant_id,
        title: r.title,
        provider_id: r.provider_id,
        user_id: Some(r.user_id),
        last_message_at: r.last_message_at.and_then(crate::state::naive_to_ts),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct AiConversationService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::AiConversationServiceHandlers for AiConversationService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiConversationResponse, StatusError> {
        let repo =
            crate::data::repos::AiConversationRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiConversationResponse {
            items: rows.into_iter().map(conv_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiConversationRequest,
    ) -> Result<AiConversation, StatusError> {
        let repo =
            crate::data::repos::AiConversationRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::get_ai_conversation_request::QueryBy
        );
        let row = repo
            .find_scoped(id)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        Ok(conv_proto(row))
    }

    /// 会话改名（标题）。
    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiConversationRequest,
    ) -> Result<Empty, StatusError> {
        let _payload = operator_of(&ctx)?;
        let repo =
            crate::data::repos::AiConversationRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        let mut a: crate::data::sys_ai_conversations::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.title {
                a.title = Set(Some(v.clone()));
            }
        }
        // 会话表无 updated_by 冗余列；updated_at 手动盖章。
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiConversationRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::ai::service::v1::delete_ai_conversation_request::QueryBy
        );
        let repo =
            crate::data::repos::AiConversationRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        repo.find_scoped(id)
            .await?
            .ok_or_else(|| not_found("ai conversation"))?;
        // 会话删除级联消息行（上游 delete cascade 同语义）。
        use sea_orm::QueryFilter as _;
        crate::data::sys_ai_messages::Entity::delete_many()
            .filter(crate::data::sys_ai_messages::Column::ConversationId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_ai_conversations::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
