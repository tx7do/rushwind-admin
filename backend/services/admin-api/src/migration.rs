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
        let notification_preference_template = EntityTables::new(
            "m20251105_000001_notification_preference_template",
            DatabaseBackend::Postgres,
        )
        .table::<data::sys_notification_preferences::Entity>()
        .table::<data::sys_notification_templates::Entity>()
        .build();
        let monitor_alert =
            EntityTables::new("m20251105_000003_monitor_alert", DatabaseBackend::Postgres)
                .table::<data::sys_monitor_alert_rules::Entity>()
                .build();
        let bi_dashboard =
            EntityTables::new("m20251105_000006_bi_dashboard", DatabaseBackend::Postgres)
                .table::<data::bi_dashboards::Entity>()
                .build();
        let print_template =
            EntityTables::new("m20251105_000007_print_template", DatabaseBackend::Postgres)
                .table::<data::print_templates::Entity>()
                .build();
        let ai = EntityTables::new("m20251105_000008_ai", DatabaseBackend::Postgres)
            .table::<data::sys_ai_providers::Entity>()
            .table::<data::sys_ai_agent_apps::Entity>()
            .table::<data::sys_ai_agent_runs::Entity>()
            .table::<data::sys_ai_conversations::Entity>()
            .table::<data::sys_ai_messages::Entity>()
            .table::<data::sys_ai_knowledge_bases::Entity>()
            .table::<data::sys_ai_docs::Entity>()
            .table::<data::sys_ai_chunks::Entity>()
            .table::<data::sys_ai_usage_logs::Entity>()
            .build();
        vec![
            Box::new(init),
            Box::new(notification),
            Box::new(ChannelWebhookColumns),
            Box::new(notification_preference_template),
            Box::new(ChannelSettingsSmsColumns),
            Box::new(monitor_alert),
            Box::new(RoleParentColumn),
            Box::new(UserLocaleColumn),
            Box::new(bi_dashboard),
            Box::new(print_template),
            Box::new(ai),
            Box::new(AiChunkVectorColumn),
            Box::new(AiSearchIndexTable),
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

/// ce8ab49 vintage 给渠道表补的 EXT settings 整包列与 SMS 出站列、发件人
/// 显示名列（PG 加列不带默认回填，存量行 NULL 的语义即"该面加列之前不存在"，
/// 与 webhook 四列的先例一致）。
struct ChannelSettingsSmsColumns;

impl MigrationName for ChannelSettingsSmsColumns {
    fn name(&self) -> &str {
        "m20251105_000002_channel_settings_sms"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ChannelSettingsSmsColumns {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE sys_notification_channels \
                 ADD COLUMN IF NOT EXISTS settings text, \
                 ADD COLUMN IF NOT EXISTS sms_provider varchar, \
                 ADD COLUMN IF NOT EXISTS sms_access_key_id varchar, \
                 ADD COLUMN IF NOT EXISTS sms_access_secret varchar, \
                 ADD COLUMN IF NOT EXISTS sms_sign_name varchar, \
                 ADD COLUMN IF NOT EXISTS sms_template_code varchar, \
                 ADD COLUMN IF NOT EXISTS sms_app_id varchar, \
                 ADD COLUMN IF NOT EXISTS sms_region varchar, \
                 ADD COLUMN IF NOT EXISTS smtp_from_name varchar",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// Applies pending migrations. Called before seeding on startup.
/// 逐步应用（每次一个迁移）：失败时日志带出"上一个成功的迁移名"，不然
/// PG 只报一句事务内行号的 Query Error，定位不到是哪个迁移的哪条 DDL。
pub async fn run(db: &sea_orm::DatabaseConnection) -> Result<(), String> {
    let total = <Migrator as MigratorTrait>::migrations().len() as u32;
    let mut last_applied = String::from("<start>");
    for _ in 0..=total {
        let applied = <Migrator as MigratorTrait>::get_applied_migrations(db)
            .await
            .map_err(|e| format!("schema migrate list: {e}"))?;
        if applied.len() as u32 >= total {
            return Ok(());
        }
        Migrator::up(db, Some(1))
            .await
            .map_err(|e| {
                eprintln!("[migrate] failed right after {last_applied}");
                format!("schema migrate: {e}")
            })?;
        let applied = <Migrator as MigratorTrait>::get_applied_migrations(db)
            .await
            .map_err(|e| format!("schema migrate list: {e}"))?;
        if let Some(latest) = applied.iter().map(|m| m.name().to_string()).max() {
            eprintln!("[migrate] applied {latest}");
            last_applied = latest;
        }
    }
    Ok(())
}

/// ce8ab49 vintage 的角色权限继承给 sys_roles 补的父角色列（NULL = 根角色）。
struct RoleParentColumn;

impl MigrationName for RoleParentColumn {
    fn name(&self) -> &str {
        "m20251105_000004_role_parent"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RoleParentColumn {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE sys_roles \
                 ADD COLUMN IF NOT EXISTS parent_id integer",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// ce8ab49 vintage 给 sys_users 补的偏好语言列（事务性邮件按此渲染，
/// 空 = 按请求 Accept-Language）。
struct UserLocaleColumn;

impl MigrationName for UserLocaleColumn {
    fn name(&self) -> &str {
        "m20251105_000005_user_locale"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for UserLocaleColumn {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE sys_users \
                 ADD COLUMN IF NOT EXISTS locale varchar",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// RAG 向量列启动补建（幂等）：`CREATE EXTENSION IF NOT EXISTS vector` +
/// `sys_ai_chunks.embedding vector(1536)` 定维列。扩展需要超级用户；任一步
/// 失败**不阻断迁移链**（RAG 功能降级，其余不受影响）——与上游
/// MigrateVectorColumn 的"失败只记日志"语义一致。历史非定维/异维度列的
/// 整列重建（向量清空 + 重索引）超出本迁移面：embedding 模型换维时手工
/// DROP 该列后重启即可再次补建。
struct AiChunkVectorColumn;

impl MigrationName for AiChunkVectorColumn {
    fn name(&self) -> &str {
        "m20251105_000009_ai_chunk_vector"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AiChunkVectorColumn {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        let conn = manager.get_connection();
        // 迁移跑在事务里：PG 事务内任一语句报错即 abort 整个事务且无法吞掉
        // 继续——"CREATE EXTENSION 失败后照常补列"在这个执行模型下不成立。
        // 因此扩展缺席（无超级用户）时整体安静跳过：RAG 降级，迁移链与
        // 服务照常；扩展在场时两条 DDL 幂等补建。
        let has_vector = conn
            .query_one_raw(sea_orm::Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM pg_extension WHERE extname = 'vector'",
            ))
            .await
            .map(|r| r.is_some())
            .unwrap_or(false);
        eprintln!("[migrate] pg_extension vector present = {has_vector}");
        if !has_vector {
            return Ok(());
        }
        conn.execute_unprepared(
            "ALTER TABLE sys_ai_chunks ADD COLUMN IF NOT EXISTS embedding vector(1536)",
        )
        .await
        .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// 菜单语义搜索索引表（与上游 buildSearchIndexSQL 同构；embedding 定维
/// 1536——混维向量会让 <=> 在查询期整体报错，定维把失败提前到写入期）。
/// 建表依赖 vector 扩展（前一迁移）；扩展缺席时本迁移静默跳过——语义
/// 检索面降级，SemanticSearch 以空索引回答。
struct AiSearchIndexTable;

impl MigrationName for AiSearchIndexTable {
    fn name(&self) -> &str {
        "m20251105_000010_ai_search_index"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AiSearchIndexTable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        // 同上：vector 类型缺席（扩展未装）时建表必报错且事务不可续——
        // 整体跳过，语义检索面降级为空索引。
        let has_vector = manager
            .get_connection()
            .query_one_raw(sea_orm::Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM pg_extension WHERE extname = 'vector'",
            ))
            .await
            .map(|r| r.is_some())
            .unwrap_or(false);
        if !has_vector {
            return Ok(());
        }
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE IF NOT EXISTS sys_ai_search_index (\
                 id BIGSERIAL PRIMARY KEY, \
                 created_at TIMESTAMPTZ DEFAULT NOW(), \
                 item_type VARCHAR(20) NOT NULL DEFAULT 'menu', \
                 item_id BIGINT NOT NULL, \
                 title TEXT NOT NULL, \
                 route TEXT NOT NULL DEFAULT '', \
                 embedding vector(1536))",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}
