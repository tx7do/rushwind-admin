//! The SeaORM entity catalog for the full schema. Enum columns carry
//! the enum names as text.

mod audit;
mod bi;
mod dict;
mod identity;
mod message;
mod monitor;
mod notification;
mod ops;
mod platform;
mod rbac;
mod scope;

pub use audit::{
    sys_api_audit_logs, sys_data_access_audit_logs, sys_login_audit_logs, sys_operation_audit_logs,
    sys_permission_audit_logs, sys_policy_evaluation_logs,
};
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
pub use rbac::{
    sys_apis, sys_menus, sys_permission_apis, sys_permission_groups, sys_permission_menus,
    sys_permissions, sys_role_field_permissions, sys_role_metadata, sys_role_org_units,
    sys_role_permissions, sys_roles,
};
pub use scope::Viewer;

pub mod repos;

/// Wall clock for timestamp columns.
pub fn now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}
