//! InternalMessageRecipientRepo — the user inbox: own rows newest
//! first, the N+1-guarding message backfill, and per-row status moves.

//! InternalMessage repos — message repos:
//! send inserts denormalized recipients (status RECEIVED immediately);
//! delete/revoke cascade recipient rows; the inbox is the user-scoped
//! recipient list with one IN-query backfill.

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, Set};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::data::{internal_message_recipients, internal_messages};
use crate::state::{db_err, StatusError};

pub struct InternalMessageRecipientRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> InternalMessageRecipientRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// The user inbox: own rows, newest first.
    pub async fn list_inbox(
        &self,
        user_id: i64,
    ) -> Result<Vec<internal_message_recipients::Model>, StatusError> {
        internal_message_recipients::Entity::find()
            .filter(internal_message_recipients::Column::RecipientUserId.eq(user_id))
            .order_by_desc(internal_message_recipients::Column::CreatedAt)
            .all(self.db)
            .await
            .map_err(db_err)
    }

    /// Paged inbox listing: returns (rows, total).
    pub async fn paged_inbox(
        &self,
        user_id: i64,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<internal_message_recipients::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            internal_message_recipients::Entity::find()
                .filter(internal_message_recipients::Column::RecipientUserId.eq(user_id))
                .order_by_desc(internal_message_recipients::Column::CreatedAt),
            req,
        )
        .await
    }

    /// One IN query backfills the parent messages (N+1 guard).
    pub async fn list_messages_by_ids(
        &self,
        ids: &[u64],
    ) -> Result<std::collections::HashMap<i64, internal_messages::Model>, StatusError> {
        if ids.is_empty() {
            return Ok(Default::default());
        }
        Ok(internal_messages::Entity::find()
            .filter(internal_messages::Column::Id.is_in(ids.to_vec()))
            .all(self.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|m| (m.id, m))
            .collect())
    }

    pub async fn mark_read(&self, user_id: i64, recipient_ids: &[u64]) -> Result<(), StatusError> {
        for id in recipient_ids {
            if let Some(row) = internal_message_recipients::Entity::find_by_id(*id as i64)
                .filter(internal_message_recipients::Column::RecipientUserId.eq(user_id))
                .one(self.db)
                .await
                .map_err(db_err)?
            {
                let mut a: internal_message_recipients::ActiveModel = row.into();
                a.status = Set(Some("READ".into()));
                a.read_at = Set(Some(crate::data::now()));
                a.updated_at = Set(Some(crate::data::now()));
                a.update(self.db).await.map_err(db_err)?;
            }
        }
        Ok(())
    }

    pub async fn mark_status(
        &self,
        user_id: i64,
        recipient_ids: &[u64],
        status: &str,
    ) -> Result<(), StatusError> {
        for id in recipient_ids {
            if let Some(row) = internal_message_recipients::Entity::find_by_id(*id as i64)
                .filter(internal_message_recipients::Column::RecipientUserId.eq(user_id))
                .one(self.db)
                .await
                .map_err(db_err)?
            {
                let mut a: internal_message_recipients::ActiveModel = row.into();
                a.status = Set(Some(status.to_string()));
                a.updated_at = Set(Some(crate::data::now()));
                a.update(self.db).await.map_err(db_err)?;
            }
        }
        Ok(())
    }

    pub async fn delete_from_inbox(
        &self,
        user_id: i64,
        recipient_ids: &[u64],
    ) -> Result<(), StatusError> {
        internal_message_recipients::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(internal_message_recipients::Column::RecipientUserId.eq(user_id))
                    .add(internal_message_recipients::Column::Id.is_in(recipient_ids.to_vec())),
            )
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
