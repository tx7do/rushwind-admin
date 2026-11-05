//! NotificationTemplateRepo — platform-global rows; `code` is the
//! unique reference anchor senders cite.

use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

use crate::data::sys_notification_templates as entity;
use crate::state::{db_err, not_found, StatusError};

repo_shell!(global NotificationTemplateRepo, entity, "notification template");

impl<'a> NotificationTemplateRepo<'a> {
    /// The row by its unique code (the senders' reference anchor).
    pub async fn get_by_code(&self, code: &str) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::Code.eq(code))
            .one(self.db)
            .await
            .map_err(db_err)
    }
}
