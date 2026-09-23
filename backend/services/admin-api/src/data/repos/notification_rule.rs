//! NotificationRuleRepo — platform-global routing rules (event_type unique).

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::sys_notification_rules as entity;
use crate::state::{db_err, StatusError};

repo_shell!(global NotificationRuleRepo, entity);

impl<'a> NotificationRuleRepo<'a> {
    pub async fn get_by_id(&self, id: u32) -> Result<entity::Model, StatusError> {
        entity::Entity::find_by_id(id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| StatusError::new(404, "NOT_FOUND", "notification rule not found"))
    }

    /// event_type 是唯一键:创建/改路由前先查重。
    pub async fn find_by_event_type(
        &self,
        event_type: &str,
    ) -> Result<Option<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::EventType.eq(event_type))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn delete_by_id(&self, id: u32) -> Result<(), StatusError> {
        entity::Entity::delete_by_id(id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
