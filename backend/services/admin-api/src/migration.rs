//! Schema bootstrap — the framework's migration facility
//! (`rushwind-storage-seaorm-migration`) driven by this service's entity
//! catalog: every table derives from the definitions in [`crate::data`],
//! applied once and tracked, at startup when the `database.migrate` gate
//! is on.

use rushwind_storage_seaorm_migration::{
    EntityTables, MigrationName, MigrationTrait, MigratorTrait, SchemaManager,
};
use sea_orm::{ConnectionTrait, DatabaseBackend};

use crate::data;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let init = EntityTables::new("m20250915_000001_init", DatabaseBackend::Postgres)
            // audit
            .table::<data::sys_api_audit_logs::Entity>()
            .table::<data::sys_data_access_audit_logs::Entity>()
            .table::<data::sys_login_audit_logs::Entity>()
            .table::<data::sys_operation_audit_logs::Entity>()
            .table::<data::sys_permission_audit_logs::Entity>()
            .table::<data::sys_policy_evaluation_logs::Entity>()
            // identity
            .table::<data::sys_org_units::Entity>()
            .table::<data::sys_plan_modules::Entity>()
            .table::<data::sys_plan_quotas::Entity>()
            .table::<data::sys_plans::Entity>()
            .table::<data::sys_positions::Entity>()
            .table::<data::sys_tenants::Entity>()
            .table::<data::sys_user_credentials::Entity>()
            .table::<data::sys_user_mfa_factors::Entity>()
            .table::<data::sys_user_roles::Entity>()
            .table::<data::sys_users::Entity>()
            // misc
            .table::<data::files::Entity>()
            .table::<data::internal_message_categories::Entity>()
            .table::<data::internal_message_recipients::Entity>()
            .table::<data::internal_messages::Entity>()
            .table::<data::sys_access_keys::Entity>()
            .table::<data::sys_configs::Entity>()
            .table::<data::sys_dict_entries::Entity>()
            .table::<data::sys_dict_entry_i18n::Entity>()
            .table::<data::sys_dict_types::Entity>()
            .table::<data::sys_languages::Entity>()
            .table::<data::sys_login_policies::Entity>()
            .table::<data::sys_notification_channels::Entity>()
            .table::<data::sys_script_logs::Entity>()
            .table::<data::sys_scripts::Entity>()
            .table::<data::sys_tasks::Entity>()
            // rbac
            .table::<data::sys_apis::Entity>()
            .table::<data::sys_menus::Entity>()
            .table::<data::sys_permission_apis::Entity>()
            .table::<data::sys_permission_groups::Entity>()
            .table::<data::sys_permission_menus::Entity>()
            .table::<data::sys_permissions::Entity>()
            .table::<data::sys_role_field_permissions::Entity>()
            .table::<data::sys_role_metadata::Entity>()
            .table::<data::sys_role_org_units::Entity>()
            .table::<data::sys_role_permissions::Entity>()
            .table::<data::sys_roles::Entity>()
            .build();
        let notification =
            EntityTables::new("m20250923_000001_notification", DatabaseBackend::Postgres)
                .table::<data::sys_notification_deliveries::Entity>()
                .table::<data::sys_notification_rules::Entity>()
                .build();
        vec![
            Box::new(init),
            Box::new(notification),
            Box::new(ChannelWebhookColumns),
        ]
    }
}

/// 上游 N/C 契约给渠道表补的 WEBHOOK 出站四列(PG 加列不带默认回填,存量行 NULL
/// 的语义即"加列之前只有 CUSTOM 一种行为",与上游注释一致)。
struct ChannelWebhookColumns;

impl MigrationName for ChannelWebhookColumns {
    fn name(&self) -> &str {
        "m20250923_000002_channel_webhook"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ChannelWebhookColumns {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE sys_notification_channels \
                 ADD COLUMN IF NOT EXISTS webhook_url varchar, \
                 ADD COLUMN IF NOT EXISTS webhook_secret varchar, \
                 ADD COLUMN IF NOT EXISTS webhook_sign_style varchar, \
                 ADD COLUMN IF NOT EXISTS webhook_payload_template varchar",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// Applies pending migrations. Called before seeding on startup.
pub async fn run(db: &sea_orm::DatabaseConnection) -> Result<(), String> {
    Migrator::up(db, None)
        .await
        .map_err(|e| format!("schema migrate: {e}"))
}
