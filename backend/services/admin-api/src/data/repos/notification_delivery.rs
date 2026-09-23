//! NotificationDeliveryRepo — platform-global delivery ledger (read-only view).

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::sys_notification_deliveries as entity;
use crate::state::{db_err, StatusError};

repo_shell!(global NotificationDeliveryRepo, entity);

impl<'a> NotificationDeliveryRepo<'a> {
    pub async fn get_by_id(&self, id: u32) -> Result<entity::Model, StatusError> {
        entity::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| StatusError::new(404, "NOT_FOUND", "notification delivery not found"))
    }

    pub async fn insert(&self, row: entity::ActiveModel) -> Result<entity::Model, StatusError> {
        ActiveModelTrait::insert(row, self.db).await.map_err(db_err)
    }
}
