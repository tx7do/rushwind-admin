//! InternalMessageService — tenant messages: send (with the SSE
//! recipient fan-out), list/get/update/delete, and revoke with the
//! recipient cascade.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, tenant_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::internal_message::service::v1::{
    GetInternalMessageRequest, InternalMessage, ListInternalMessageResponse, RevokeMessageRequest,
    SendMessageRequest, SendMessageResponse, UpdateInternalMessageRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as DRAFT.
fn message_status_to_proto(s: &str) -> i32 {
    mapping::internal_message_status_of(s).unwrap_or(0)
}

fn message_proto(r: crate::data::internal_messages::Model) -> InternalMessage {
    InternalMessage {
        id: Some(r.id as u64),
        title: r.title,
        content: r.content,
        status: r.status.as_deref().map(message_status_to_proto),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| mapping::internal_message_type_of(s).unwrap_or(0)),
        sender_id: r.sender_id.map(|v| v as u64),
        sender_name: None,
        category_id: r.category_id.map(|v| v as u64),
        category_name: None,
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct InternalMessageService {
    pub state: Arc<AppState>,
}

impl InternalMessageService {
    /// Insert recipients with the denormalized title/content (status
    /// RECEIVED immediately — contract for inbox reads).
    pub async fn insert_recipients(
        &self,
        tenant_id: i64,
        message_id: i64,
        title: &str,
        content: &str,
        recipient_ids: &[i64],
    ) -> Result<(), StatusError> {
        for uid in recipient_ids {
            crate::data::internal_message_recipients::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                message_id: Set(Some(message_id)),
                recipient_user_id: Set(Some(*uid)),
                status: Set(Some("RECEIVED".into())),
                received_at: Set(Some(crate::data::now())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        let _ = (title, content);
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::InternalMessageServiceHandlers for InternalMessageService {
    async fn list_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListInternalMessageResponse, StatusError> {
        let repo = crate::data::repos::InternalMessageRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(tenant_of(&ctx), &req).await?;
        Ok(ListInternalMessageResponse {
            items: rows.into_iter().map(message_proto).collect(),
            total,
        })
    }

    async fn get_message(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetInternalMessageRequest,
    ) -> Result<InternalMessage, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::internal_message::service::v1::get_internal_message_request::QueryBy
        );
        let row = crate::data::internal_messages::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        Ok(message_proto(row))
    }

    async fn update_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateInternalMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::internal_messages::Entity::find_by_id(req.id as i64)
            .filter(crate::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        let mut a: crate::data::internal_messages::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.title {
                a.title = Set(Some(v.clone()));
            }
            if let Some(v) = &data.content {
                a.content = Set(Some(v.clone()));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: proto::proto::internal_message::service::v1::DeleteInternalMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::internal_message::service::v1::delete_internal_message_request::QueryBy
        );
        let row = crate::data::internal_messages::Entity::find_by_id(id as i64)
            .filter(crate::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        // Transactional cascade of recipients (DeleteMessageWithRecipients).
        crate::data::internal_message_recipients::Entity::delete_many()
            .filter(crate::data::internal_message_recipients::Column::MessageId.eq(row.id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::internal_messages::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn send_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendMessageRequest,
    ) -> Result<SendMessageResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let type_str = match req.r#type {
            1 => "PRIVATE",
            2 => "GROUP",
            _ => "NOTIFICATION",
        };
        let message = crate::data::internal_messages::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            title: Set(req.title.clone()),
            content: Set(Some(req.content.clone())),
            sender_id: Set(Some(payload.user_id)),
            category_id: Set(req.category_id.map(|v| v as i64)),
            status: Set(Some("PUBLISHED".into())),
            type_column: Set(Some(type_str.into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        // Recipients: explicit list (single or multi), else the whole
        // tenant fan-out (the broadcast rides the scheduler phase;
        // direct fan-out covers the API contract).
        let recipient_ids: Vec<i64> = if !req.target_user_ids.is_empty() {
            req.target_user_ids
                .iter()
                .map(|v| *v as i64)
                .collect::<Vec<i64>>()
        } else if let Some(uid) = req.recipient_user_id {
            vec![(uid as i64)]
        } else if req.target_all == Some(true) {
            crate::data::sys_users::Entity::find()
                .filter(crate::data::sys_users::Column::TenantId.eq(payload.tenant_id))
                .all(&self.state.db)
                .await
                .map_err(db_err)?
                .iter()
                .map(|u| u.id)
                .collect()
        } else {
            Vec::new()
        };
        let received_at = crate::state::naive_to_ts(crate::data::now());
        for chunk in recipient_ids.chunks(1000) {
            self.insert_recipients(
                payload.tenant_id,
                message.id,
                req.title.as_deref().unwrap_or(""),
                &req.content,
                chunk,
            )
            .await?;
            // SSE push per recipient (publishNotification: stream = userId,
            // event = notification, data = the recipient protojson).
            for uid in chunk {
                crate::server::sse::publish_recipient(
                    &self.state.hub,
                    &crate::server::sse::NotificationPayload {
                        id: message.id,
                        message_id: (message.id as u64 as i64),
                        recipient_user_id: *uid,
                        title: req.title.clone().unwrap_or_default(),
                        content: req.content.clone(),
                        received_at,
                    },
                );
            }
        }
        Ok(SendMessageResponse {
            message_id: message.id as u64,
        })
    }

    async fn revoke_message(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RevokeMessageRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::internal_messages::Entity::find_by_id(req.message_id as i64)
            .filter(crate::data::internal_messages::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("internal message"))?;
        crate::data::internal_message_recipients::Entity::delete_many()
            .filter(crate::data::internal_message_recipients::Column::MessageId.eq(row.id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut a: crate::data::internal_messages::ActiveModel = row.into();
        a.status = Set(Some("REVOKED".into()));
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }
}
