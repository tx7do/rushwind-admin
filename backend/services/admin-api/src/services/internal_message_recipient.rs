//! InternalMessageRecipientService — the user inbox: paged reads
//! with the one-query message backfill, mark-read/status, and
//! inbox delete.

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::mapping;
use crate::state::{db_err, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::internal_message::service::v1::{
    DeleteNotificationFromInboxRequest, InternalMessageRecipient, ListUserInboxResponse,
    MarkNotificationAsReadRequest, MarkNotificationsStatusRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as RECEIVED.
fn recipient_status_to_proto(s: &str) -> i32 {
    mapping::internal_message_recipient_status_of(s).unwrap_or(1)
}

pub struct InternalMessageRecipientService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::InternalMessageRecipientServiceHandlers
    for InternalMessageRecipientService
{
    async fn list_user_inbox(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListUserInboxResponse, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::InternalMessageRecipientRepo::new(&self.state.db);
        let (rows, total) = repo.paged_inbox(payload.user_id, &req).await?;
        // One IN query backfills the messages (N+1 guard).
        let message_ids: Vec<i64> = rows.iter().filter_map(|r| r.message_id).collect();
        let messages: std::collections::HashMap<i64, crate::data::internal_messages::Model> =
            if message_ids.is_empty() {
                Default::default()
            } else {
                crate::data::internal_messages::Entity::find()
                    .filter(crate::data::internal_messages::Column::Id.is_in(message_ids))
                    .all(&self.state.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| (m.id, m))
                    .collect()
            };
        let items: Vec<InternalMessageRecipient> = rows
            .into_iter()
            .map(|r| {
                let message = r.message_id.and_then(|mid| messages.get(&mid));
                InternalMessageRecipient {
                    id: Some(r.id as u64),
                    recipient_user_id: r.recipient_user_id.map(|v| v as u64),
                    message_id: r.message_id.map(|v| v as u64),
                    status: r.status.as_deref().map(recipient_status_to_proto),
                    received_at: r.received_at.and_then(crate::state::naive_to_ts),
                    read_at: r.read_at.and_then(crate::state::naive_to_ts),
                    // Denormalized title/content copied at send time; the
                    // backfilled message row fills gaps.
                    title: message.and_then(|m| m.title.clone()),
                    content: message.and_then(|m| m.content.clone()),
                    tenant_id: r.tenant_id.map(|v| v as u64),
                    tenant_name: None,
                    created_by: r.created_by.map(|v| v as u64),
                    updated_by: r.updated_by.map(|v| v as u64),
                    deleted_by: r.deleted_by.map(|v| v as u64),
                    created_at: r.created_at.and_then(crate::state::naive_to_ts),
                    updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
                    deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
                }
            })
            .collect();
        Ok(ListUserInboxResponse { items, total })
    }

    async fn delete_notification_from_inbox(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationFromInboxRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        crate::data::internal_message_recipients::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(
                        crate::data::internal_message_recipients::Column::RecipientUserId
                            .eq(payload.user_id),
                    )
                    .add(
                        crate::data::internal_message_recipients::Column::Id
                            .is_in(req.recipient_ids),
                    ),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn mark_notification_as_read(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: MarkNotificationAsReadRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        crate::data::internal_message_recipients::Entity::update_many()
            .col_expr(
                crate::data::internal_message_recipients::Column::Status,
                sea_orm::sea_query::Expr::value("READ"),
            )
            .col_expr(
                crate::data::internal_message_recipients::Column::ReadAt,
                sea_orm::sea_query::Expr::value(crate::data::now()),
            )
            .filter(
                Condition::all()
                    .add(
                        crate::data::internal_message_recipients::Column::RecipientUserId
                            .eq(payload.user_id),
                    )
                    .add(
                        crate::data::internal_message_recipients::Column::Id
                            .is_in(req.recipient_ids),
                    ),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn mark_notifications_status(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: MarkNotificationsStatusRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let status = match req.new_status {
            2 => "READ",
            4 => "DELETED",
            _ => "RECEIVED",
        };
        crate::data::internal_message_recipients::Entity::update_many()
            .col_expr(
                crate::data::internal_message_recipients::Column::Status,
                sea_orm::sea_query::Expr::value(status),
            )
            .filter(
                Condition::all()
                    .add(
                        crate::data::internal_message_recipients::Column::RecipientUserId
                            .eq(payload.user_id),
                    )
                    .add(
                        crate::data::internal_message_recipients::Column::Id
                            .is_in(req.recipient_ids),
                    ),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
