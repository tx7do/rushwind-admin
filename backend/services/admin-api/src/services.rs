//! The service layer — one module per proto service, each implementing
//! its generated Handlers trait against the shared [`crate::state::AppState`]

mod access_key;
mod admin_portal;
mod api;
mod api_audit_log;
mod authentication;
mod config;
mod dashboard;
mod data_access_audit_log;
mod dict_entry;
mod dict_type;
mod file;
mod internal_message;
mod language;
mod login_audit_log;
mod login_policy;
mod menu;
mod mfa;
mod monitor;
mod notification_channel;
mod notification_delivery;
mod notification_rule;
mod online_session;
mod operation_audit_log;
mod org_unit;
mod permission;
mod permission_audit_log;
mod permission_group;
mod plan;
mod policy_evaluation_log;
mod position;
mod role;
mod script;
mod task;
mod tenant;
mod user;
mod user_profile;

pub use access_key::AccessKeyService;
pub use admin_portal::{
    load_user, menu_meta_from_json, menu_meta_to_json, user_to_proto, AdminPortalService,
};
pub use api::ApiService;
pub use api_audit_log::ApiAuditLogService;
pub use authentication::AuthenticationService;
pub use config::ConfigService;
pub use dashboard::DashboardService;
pub use data_access_audit_log::DataAccessAuditLogService;
pub use dict_entry::DictEntryService;
pub use dict_type::DictTypeService;
pub use file::{image_proxy, FileService, FileTransferService};
pub use internal_message::{
    InternalMessageCategoryService, InternalMessageRecipientService, InternalMessageService,
};
pub use language::LanguageService;
pub use login_audit_log::LoginAuditLogService;
pub use login_policy::LoginPolicyService;
pub use menu::MenuService;
pub use mfa::MfaService;
pub use monitor::{RedisCacheMonitorService, ServerMonitorService};
pub use notification_channel::NotificationChannelService;
pub use notification_delivery::NotificationDeliveryService;
pub use notification_rule::NotificationRuleService;
pub use online_session::OnlineSessionService;
pub use operation_audit_log::OperationAuditLogService;
pub use org_unit::OrgUnitService;
pub use permission::PermissionService;
pub use permission_audit_log::PermissionAuditLogService;
pub use permission_group::PermissionGroupService;
pub use plan::{PlanModuleService, PlanQuotaService, PlanService};
pub use policy_evaluation_log::PolicyEvaluationLogService;
pub use position::PositionService;
pub use role::RoleService;
pub use script::{ScriptLogService, ScriptService};
pub use task::TaskService;
pub use tenant::TenantService;
pub use user::UserService;
pub use user_profile::UserProfileService;
