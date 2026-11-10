//! The SeaORM entity catalog for the full schema. Enum columns carry
//! the enum names as text.

mod ai;
mod audit;
mod bi;
mod dict;
mod identity;
mod message;
mod monitor;
mod notification;
mod ops;
mod platform;
mod print;
mod rbac;
mod scope;

pub use ai::{
    sys_ai_agent_apps, sys_ai_agent_revisions, sys_ai_agent_runs, sys_ai_chunks,
    sys_ai_conversations, sys_ai_docs, sys_ai_knowledge_bases, sys_ai_messages, sys_ai_providers,
    sys_ai_usage_logs,
};
pub use audit::{
    sys_api_audit_logs, sys_data_access_audit_logs, sys_login_audit_logs, sys_operation_audit_logs,
    sys_permission_audit_logs, sys_policy_evaluation_logs,
};
pub use bi::bi_dashboard_revisions;
pub use bi::bi_dashboards;
pub use dict::{sys_dict_entries, sys_dict_entry_i18n, sys_dict_types, sys_languages};
pub use identity::{
    sys_org_units, sys_plan_modules, sys_plan_quotas, sys_plans, sys_positions, sys_tenants,
    sys_user_credentials, sys_user_mfa_factors, sys_user_roles, sys_users,
};
pub use message::{internal_message_categories, internal_message_recipients, internal_messages};
pub use monitor::sys_monitor_alert_rules;
pub use notification::{
    sys_notification_channels, sys_notification_deliveries, sys_notification_preferences,
    sys_notification_rules, sys_notification_templates,
};
pub use ops::{sys_script_logs, sys_scripts, sys_tasks};
pub use platform::{files, sys_access_keys, sys_configs, sys_login_policies};
pub use print::print_templates;
pub use rbac::{
    sys_apis, sys_menus, sys_permission_apis, sys_permission_groups, sys_permission_menus,
    sys_permissions, sys_role_field_permissions, sys_role_metadata, sys_role_org_units,
    sys_role_permissions, sys_roles,
};
pub use scope::Viewer;

/// 子树收集（上游 collectTreeIDs 同语义）：内存 parent→children 映射 +
/// visited 防护——存量环脏数据正常收敛不打转。委托 `rust_utils::sliceutil`
/// （环收敛 doctest 落在库侧），本仓保留转发以稳定数十个调用点。
pub fn collect_subtree_ids<T: Copy + Eq + std::hash::Hash>(
    rows: &[(T, Option<T>)],
    root: T,
) -> Vec<T> {
    rust_utils::sliceutil::collect_subtree_ids(rows, root)
}

pub mod repos;

/// Wall clock for timestamp columns.
pub fn now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}
