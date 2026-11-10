//! InternalMessageRepo — the admin message surface: tenant-scoped
//! listing and the recipient cascade delete.

//! InternalMessage repos — message repos:
//! send inserts denormalized recipients (status RECEIVED immediately);
//! delete/revoke cascade recipient rows; the inbox is the user-scoped
//! recipient list with one IN-query backfill.

use sea_orm::{ActiveModelTrait, Set};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::data::{internal_message_recipients, internal_messages};
use crate::state::{db_err, StatusError};

pub struct InternalMessageRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> InternalMessageRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// SendMessage's recipient fan-out: denormalized title/content rows
    /// with status RECEIVED immediately (never uses SENT).
    pub async fn insert_recipients(
        &self,
        tenant_id: i64,
        message_id: i64,
        recipient_ids: &[u64],
    ) -> Result<(), StatusError> {
        for uid in recipient_ids {
            internal_message_recipients::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                message_id: Set(Some(message_id)),
                recipient_user_id: Set(Some(*uid as i64)),
                status: Set(Some("RECEIVED".into())),
                received_at: Set(Some(crate::data::now())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(self.db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// Paged tenant-scoped listing: returns (rows, total).
    pub async fn paged_list(
        &self,
        tenant_id: i64,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<internal_messages::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            internal_messages::Entity::find()
                .filter(internal_messages::Column::TenantId.eq(tenant_id))
                .order_by_desc(internal_messages::Column::CreatedAt),
            req,
        )
        .await
    }

    /// DeleteMessageWithRecipients / RevokeMessageWithRecipients.
    pub async fn delete_recipients(&self, message_id: i64) -> Result<(), StatusError> {
        internal_message_recipients::Entity::delete_many()
            .filter(internal_message_recipients::Column::MessageId.eq(message_id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
