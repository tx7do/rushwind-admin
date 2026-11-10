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
        let bi_revisions =
            EntityTables::new("m20251105_000011_bi_revisions", DatabaseBackend::Postgres)
                .table::<data::bi_dashboard_revisions::Entity>()
                .build();
        let ai_agent_revisions = EntityTables::new(
            "m20251105_000015_ai_agent_revisions",
            DatabaseBackend::Postgres,
        )
        .table::<data::sys_ai_agent_revisions::Entity>()
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
            Box::new(bi_revisions),
            Box::new(BiRevisionIndexes),
            Box::new(ai_agent_revisions),
            Box::new(AiAgentRevisionIndexes),
            Box::new(ApiEndpointIdentity),
            Box::new(PermissionMenuFullPath),
            Box::new(IdWidenU64),
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
        Migrator::up(db, Some(1)).await.map_err(|e| {
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

/// 版本快照表的两条索引（上游 uidx_bi_revisions_dash_version /
/// idx_bi_revisions_dash_created）：一张仪表盘一个版本号只留一份快照
/// （保存/恢复去重），历史列表按仪表盘拉取、新快照在前。
struct BiRevisionIndexes;

impl MigrationName for BiRevisionIndexes {
    fn name(&self) -> &str {
        "m20251105_000012_bi_revision_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for BiRevisionIndexes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX IF NOT EXISTS uidx_bi_revisions_dash_version \
                 ON sys_bi_dashboard_revisions (dashboard_id, version); \
                 CREATE INDEX IF NOT EXISTS idx_bi_revisions_dash_created \
                 ON sys_bi_dashboard_revisions (dashboard_id, created_at)",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// 授权绑定身份迁移（上游 7eb7f9f0 同语义）：绑定键从 api_id 迁到
/// (path, method) 端点快照。①预去重 sys_apis（同 (path,method) 保留最小
/// id——旧唯一键 (module,path,method,scope) 下同端点可重复，新唯一索引
/// 会被脏数据卡死建链）；②绑定表加快照列；③按 api_id→sys_apis 回填；
/// ④sys_apis 建 (path,method) 唯一索引。全部幂等，可重复执行。
struct ApiEndpointIdentity;

impl MigrationName for ApiEndpointIdentity {
    fn name(&self) -> &str {
        "m20251105_000013_api_endpoint_identity"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ApiEndpointIdentity {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            // 预去重：NULL/空身份行不是任何端点，不参与去重（GROUP BY 会把
            // NULL 归成一组，直接对全表 NOT IN 会把它们误删——上游同注释）
            "DELETE FROM sys_apis \
             WHERE path IS NOT NULL AND path <> '' AND method IS NOT NULL AND method <> '' \
             AND id NOT IN ( \
               SELECT MIN(id) FROM sys_apis \
               WHERE path IS NOT NULL AND path <> '' AND method IS NOT NULL AND method <> '' \
               GROUP BY path, method)",
        )
        .await?;
        conn.execute_unprepared(
            "ALTER TABLE sys_permission_apis \
             ADD COLUMN IF NOT EXISTS endpoint_path varchar(255), \
             ADD COLUMN IF NOT EXISTS endpoint_method varchar(16)",
        )
        .await?;
        conn.execute_unprepared(
            "UPDATE sys_permission_apis p \
             SET endpoint_path = a.path, endpoint_method = a.method \
             FROM sys_apis a \
             WHERE p.api_id = a.id \
               AND (p.endpoint_path IS NULL OR p.endpoint_method IS NULL)",
        )
        .await?;
        conn.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_sys_apis_path_method \
             ON sys_apis (path, method); \
             CREATE INDEX IF NOT EXISTS idx_perm_api_endpoint \
             ON sys_permission_apis (endpoint_path, endpoint_method)",
        )
        .await
        .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// 菜单授权绑定键迁移（上游 836334e3 同语义）：menu_id → 全路径快照。
/// 回填按父链递归 CTE 拼全路径（与派生口径一致：parent_path + path 直连）。
struct PermissionMenuFullPath;

impl MigrationName for PermissionMenuFullPath {
    fn name(&self) -> &str {
        "m20251105_000014_permission_menu_full_path"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for PermissionMenuFullPath {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "ALTER TABLE sys_permission_menus \
             ADD COLUMN IF NOT EXISTS full_path varchar(512)",
        )
        .await?;
        conn.execute_unprepared(
            "WITH RECURSIVE walk AS ( \
               SELECT id, path, parent_id, path AS full_path \
                 FROM sys_menus WHERE parent_id IS NULL \
               UNION ALL \
               SELECT m.id, m.path, m.parent_id, w.full_path || m.path \
                 FROM sys_menus m JOIN walk w ON m.parent_id = w.id \
             ) \
             UPDATE sys_permission_menus pm \
                SET full_path = w.full_path \
               FROM walk w \
              WHERE pm.menu_id = w.id \
                AND (pm.full_path IS NULL OR pm.full_path = '')",
        )
        .await?;
        conn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_perm_menu_full_path \
             ON sys_permission_menus (permission_id, full_path)",
        )
        .await
        .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// Agent 版本快照表的索引（上游 schema 索引同构）：历史列表按应用拉取、
/// 新快照在前。
struct AiAgentRevisionIndexes;

impl MigrationName for AiAgentRevisionIndexes {
    fn name(&self) -> &str {
        "m20251105_000016_ai_agent_revision_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AiAgentRevisionIndexes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_ai_agent_revisions_app_created \
                 ON sys_ai_agent_revisions (app_id, created_at)",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

/// b5b336d9 的 ID 全链升级：实体标识位 u32→u64，Postgres 列型 int/serial
/// → bigint/bigserial。实体驱动的建表在新库上天然 bigint；本迁移把存量库
/// 的同名列加宽（int→bigint 是纯加宽，值域无损；serial 的 nextval 默认值
/// 与 int8 序列不受影响）。对已是 bigint 的列是 no-op 重校验，新库重放安全。
struct IdWidenU64;

impl MigrationName for IdWidenU64 {
    fn name(&self) -> &str {
        "m20251110_000001_id_widen_u64"
    }
}

/// (table, id-ish columns) — 从实体定义导出（与 crate::data 的 u64 字段
/// 一一对应；度量/序号列不在列）。
const ID_WIDEN_TABLES: &[(&str, &[&str])] = &[
    (
        "sys_ai_providers",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_ai_agent_apps",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_ai_agent_runs",
        &["id", "tenant_id", "app_id", "user_id"],
    ),
    (
        "sys_ai_conversations",
        &[
            "id",
            "tenant_id",
            "provider_id",
            "user_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_ai_messages",
        &["id", "tenant_id", "conversation_id", "user_id"],
    ),
    (
        "sys_ai_knowledge_bases",
        &["id", "tenant_id", "provider_id", "user_id"],
    ),
    ("sys_ai_docs", &["id", "tenant_id", "base_id", "user_id"]),
    ("sys_ai_chunks", &["id", "tenant_id", "doc_id"]),
    (
        "sys_ai_usage_logs",
        &[
            "id",
            "tenant_id",
            "provider_id",
            "conversation_id",
            "user_id",
        ],
    ),
    (
        "sys_ai_agent_revisions",
        &["id", "tenant_id", "app_id", "created_by"],
    ),
    ("sys_api_audit_logs", &["id", "tenant_id", "user_id"]),
    ("sys_operation_audit_logs", &["id", "tenant_id", "user_id"]),
    ("sys_login_audit_logs", &["id", "tenant_id", "user_id"]),
    (
        "sys_data_access_audit_logs",
        &["id", "tenant_id", "user_id"],
    ),
    (
        "sys_permission_audit_logs",
        &["id", "tenant_id", "operator_id"],
    ),
    (
        "sys_policy_evaluation_logs",
        &[
            "id",
            "tenant_id",
            "user_id",
            "membership_id",
            "permission_id",
            "policy_id",
        ],
    ),
    (
        "bi_dashboards",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_bi_dashboard_revisions",
        &["id", "tenant_id", "dashboard_id", "created_by"],
    ),
    (
        "sys_dict_types",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_dict_entries",
        &[
            "id",
            "tenant_id",
            "type_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_dict_entry_i18n",
        &[
            "id",
            "tenant_id",
            "entry_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_languages",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_users",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_user_credentials",
        &[
            "id",
            "tenant_id",
            "user_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_user_mfa_factors",
        &[
            "id",
            "tenant_id",
            "user_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_user_roles",
        &[
            "id",
            "tenant_id",
            "user_id",
            "role_id",
            "assigned_by",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_org_units",
        &[
            "id",
            "tenant_id",
            "parent_id",
            "leader_id",
            "legal_entity_org_id",
            "contact_user_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_positions",
        &[
            "id",
            "tenant_id",
            "org_unit_id",
            "reports_to_position_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_tenants",
        &[
            "id",
            "admin_user_id",
            "plan_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_plans",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_plan_modules",
        &["id", "plan_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_plan_quotas",
        &["id", "plan_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "internal_messages",
        &[
            "id",
            "tenant_id",
            "sender_id",
            "category_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "internal_message_categories",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "internal_message_recipients",
        &[
            "id",
            "tenant_id",
            "message_id",
            "recipient_user_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "monitor_alert_rules",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_notification_channels",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_notification_rules",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_notification_deliveries",
        &[
            "id",
            "channel_id",
            "recipient_user_id",
            "related_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "notification_preferences",
        &["id", "user_id", "created_by", "updated_by"],
    ),
    (
        "notification_templates",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_tasks",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_scripts",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    ("sys_script_logs", &["id", "script_id"]),
    (
        "sys_access_keys",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_configs",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_login_policies",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "files",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "print_templates",
        &["id", "tenant_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_roles",
        &[
            "id",
            "tenant_id",
            "parent_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_role_permissions",
        &[
            "id",
            "tenant_id",
            "role_id",
            "permission_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_role_metadata",
        &[
            "id",
            "tenant_id",
            "role_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_permissions",
        &["id", "group_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_permission_groups",
        &["id", "parent_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_permission_apis",
        &[
            "id",
            "permission_id",
            "api_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_permission_menus",
        &[
            "id",
            "permission_id",
            "menu_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_menus",
        &["id", "parent_id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_apis",
        &["id", "created_by", "updated_by", "deleted_by"],
    ),
    (
        "sys_role_org_units",
        &[
            "id",
            "tenant_id",
            "role_id",
            "org_unit_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
    (
        "sys_role_field_permissions",
        &[
            "id",
            "tenant_id",
            "role_id",
            "created_by",
            "updated_by",
            "deleted_by",
        ],
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for IdWidenU64 {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        let conn = manager.get_connection();
        for (table, columns) in ID_WIDEN_TABLES {
            let alters = columns
                .iter()
                .map(|c| format!("ALTER COLUMN {c} TYPE bigint"))
                .collect::<Vec<_>>()
                .join(", ");
            conn.execute_unprepared(&format!("ALTER TABLE {table} {alters}"))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        // 加宽不回退：u64 契约是前进方向。
        Ok(())
    }
}
