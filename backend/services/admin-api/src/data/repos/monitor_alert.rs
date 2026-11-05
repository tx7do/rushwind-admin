//! MonitorAlertRepo — platform-global alert rules; the evaluator's
//! state columns (last_firing/last_value/last_alerted_at) ride the same
//! shell.

use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

use crate::data::sys_monitor_alert_rules as entity;
use crate::state::{db_err, not_found, StatusError};

repo_shell!(global MonitorAlertRepo, entity, "monitor alert rule");

impl<'a> MonitorAlertRepo<'a> {
    /// All enabled rules, for the evaluator's sweep.
    pub async fn list_enabled(&self) -> Result<Vec<entity::Model>, StatusError> {
        entity::Entity::find()
            .filter(entity::Column::IsEnabled.eq(true))
            .all(self.db)
            .await
            .map_err(db_err)
    }
}
