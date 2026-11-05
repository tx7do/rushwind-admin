//! NotificationPreferenceRepo — one row per user; the access anchor is
//! user_id alone (preferences deliberately carry no tenant column).

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::data::sys_notification_preferences as entity;
use crate::state::{db_err, StatusError};

pub struct NotificationPreferenceRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> NotificationPreferenceRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    /// The user's row, if they ever saved one.
    pub async fn get_by_user_id(&self, user_id: u32) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::UserId.eq(user_id))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    /// Insert-or-update the user's row from a full-field submit; the
    /// caller owns validation (quiet-window arithmetic, category cap).
    pub async fn upsert(
        &self,
        user_id: u32,
        quiet_enabled: bool,
        quiet_start_minute: i32,
        quiet_end_minute: i32,
        muted_category_ids: Option<sea_orm::prelude::Json>,
        operator_id: u32,
    ) -> Result<entity::Model, StatusError> {
        let existing = self.get_by_user_id(user_id).await?;
        match existing {
            Some(row) => {
                let mut a: entity::ActiveModel = row.into();
                a.quiet_enabled = Set(Some(quiet_enabled));
                a.quiet_start_minute = Set(Some(quiet_start_minute));
                a.quiet_end_minute = Set(Some(quiet_end_minute));
                a.muted_category_ids = Set(muted_category_ids);
                a.updated_by = Set(Some(operator_id));
                a.updated_at = Set(Some(crate::data::now()));
                a.update(self.db).await.map_err(db_err)
            }
            None => entity::ActiveModel {
                user_id: Set(user_id),
                quiet_enabled: Set(Some(quiet_enabled)),
                quiet_start_minute: Set(Some(quiet_start_minute)),
                quiet_end_minute: Set(Some(quiet_end_minute)),
                muted_category_ids: Set(muted_category_ids),
                created_by: Set(Some(operator_id)),
                updated_by: Set(Some(operator_id)),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(self.db)
            .await
            .map_err(db_err),
        }
    }
}
