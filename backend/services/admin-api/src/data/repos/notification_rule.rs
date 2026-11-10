//! NotificationRuleRepo — platform-global routing rules (event_type unique).

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::data::sys_notification_rules as entity;
use crate::state::{db_err, StatusError};

repo_shell!(global NotificationRuleRepo, entity, "notification rule");

impl<'a> NotificationRuleRepo<'a> {
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
}
