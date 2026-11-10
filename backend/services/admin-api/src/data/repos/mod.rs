//! Repository layer. The repo surface is the data-layer API: shared
//! listing/paging envelopes, tenancy via the [`Viewer`], and every
//! predicate a service needs more than once. Id-addressed reads and
//! writes go through the repos too (`get_by_id`/`delete_by_id`).
//!
//! Two scoped idioms coexist by design:
//!
//! * viewer-scoped — the `repo_shell!` arms derive the tenant
//!   predicate from the [`Viewer`] (tenant viewers constrained,
//!   platform/system wide);
//! * explicitly parameterized — repos like [`UserRepo`] and
//!   [`InternalMessageRepo`] take a `tenant_id`/`user_id` argument
//!   where the service, not the viewer, names the scope (an admin
//!   listing another tenant's users).
//!
//! One-off single-table reads inside a service handler are tolerated;
//! compound or reused predicates belong here.

/// The uniform repository shell: connection state, the table predicate,
/// and the two listing envelopes. Repo-specific methods live in the
/// file's own `impl` block beside the invocation. Exported for the
/// business modules: a downstream module crate derives its repos from
/// the same shell (see `business/module-template` for the consumer
/// face).
///
/// * `tenant` arm — the tenancy predicate from the viewer (tenant
///   viewers constrained, platform/system wide);
/// * `global` arm — platform-global tables, no tenant predicate and no
///   viewer state at all.
///
/// The noun arms (`global $name, $entity, "noun"`) additionally
/// generate `get_by_id`/`delete_by_id` whose 404 message is
/// `"<noun> not found"` via the shared helper. There is deliberately
/// no tenant noun arm: a tenant-scoped get/delete must carry the
/// viewer predicate, and those stay hand-written beside the
/// invocation.
///
/// The expansion is self-contained: every shared name resolves through
/// `$crate` (the admin-api library) or `sea_orm` — the expanding crate
/// keeps `sea_orm` as an extern dependency (the workspace pins one
/// version), and imports nothing else beside the macro.
#[macro_export]
macro_rules! repo_shell {
    (tenant $name:ident, $entity:ident) => {
        pub struct $name<'a> {
            pub db: &'a sea_orm::DatabaseConnection,
            pub viewer: $crate::data::Viewer,
        }

        impl<'a> $name<'a> {
            pub fn new(
                db: &'a sea_orm::DatabaseConnection,
                viewer: $crate::data::Viewer,
            ) -> Self {
                Self { db, viewer }
            }

            /// The tenancy predicate: tenant viewers are constrained,
            /// platform/system viewers see all rows.
            fn condition(&self) -> sea_orm::Condition {
                match self.viewer.tenant_scope() {
                    Some(tid) => {
                        sea_orm::Condition::all().add($entity::Column::TenantId.eq(tid))
                    }
                    None => sea_orm::Condition::all(),
                }
            }

            /// Unpaged listing.
            pub async fn list(
                &self,
            ) -> Result<Vec<$entity::Model>, $crate::state::StatusError> {
                $entity::Entity::find()
                    .filter(self.condition())
                    .all(self.db)
                    .await
                    .map_err($crate::state::db_err)
            }

            /// Paged listing over the PagingRequest contract: returns (rows, total).
            pub async fn paged_list(
                &self,
                req: &$crate::proto::proto::pagination::PagingRequest,
            ) -> Result<(Vec<$entity::Model>, u64), $crate::state::StatusError> {
                $crate::paging::fetch_paged(
                    self.db,
                    $entity::Entity::find().filter(self.condition()),
                    req,
                )
                .await
            }
        }
    };
    (global $name:ident, $entity:ident) => {
        pub struct $name<'a> {
            pub db: &'a sea_orm::DatabaseConnection,
        }

        impl<'a> $name<'a> {
            pub fn new(db: &'a sea_orm::DatabaseConnection) -> Self {
                Self { db }
            }

            // Platform-global table: no tenant predicate applies.
            fn condition(&self) -> sea_orm::Condition {
                sea_orm::Condition::all()
            }

            /// Unpaged listing.
            pub async fn list(
                &self,
            ) -> Result<Vec<$entity::Model>, $crate::state::StatusError> {
                $entity::Entity::find()
                    .filter(self.condition())
                    .all(self.db)
                    .await
                    .map_err($crate::state::db_err)
            }

            /// Paged listing over the PagingRequest contract: returns (rows, total).
            pub async fn paged_list(
                &self,
                req: &$crate::proto::proto::pagination::PagingRequest,
            ) -> Result<(Vec<$entity::Model>, u64), $crate::state::StatusError> {
                $crate::paging::fetch_paged(
                    self.db,
                    $entity::Entity::find().filter(self.condition()),
                    req,
                )
                .await
            }
        }
    };
    // The noun arm reuses the plain global shell and appends the two
    // id-addressed methods every platform-global table shares.
    (global $name:ident, $entity:ident, $noun:literal) => {
        $crate::repo_shell!(global $name, $entity);

        impl<'a> $name<'a> {
            /// The row by id: 404 when absent.
            pub async fn get_by_id(
                &self,
                id: i64,
            ) -> Result<$entity::Model, $crate::state::StatusError> {
                $entity::Entity::find_by_id(id as i64)
                    .one(self.db)
                    .await
                    .map_err($crate::state::db_err)?
                    .ok_or_else(|| $crate::state::not_found($noun))
            }

            /// Delete by id; callers decide whether absence is an error.
            pub async fn delete_by_id(
                &self,
                id: i64,
            ) -> Result<(), $crate::state::StatusError> {
                $entity::Entity::delete_by_id(id as i64)
                    .exec(self.db)
                    .await
                    .map_err($crate::state::db_err)?;
                Ok(())
            }
        }
    };
}
// Textual scope: every repo module below sees the macro (same pattern as
// the audit-suite macros inside their file).

mod access_key;
mod ai;
mod api;
mod audit;
mod bi_dashboard;
mod config;
mod dict_entry;
mod dict_type;
mod file;
mod internal_message;
mod internal_message_category;
mod internal_message_recipient;
mod language;
mod login_policy;
mod menu;
mod monitor_alert;
mod notification_channel;
mod notification_delivery;
mod notification_preference;
mod notification_rule;
mod notification_template;
mod org_unit;
mod permission;
mod permission_group;
mod plan;
mod plan_module;
mod plan_quota;
mod position;
mod print_template;
mod role;
mod script;
mod script_log;
mod task;
mod tenant;
mod user;

pub use access_key::AccessKeyRepo;
pub use ai::{
    AiAgentRevisionRepo, AiConversationRepo, AiMessageRepo, AiProviderRepo, AiUsageLogRepo,
};
pub use api::ApiRepo;
pub use audit::AuditRepo;
pub use bi_dashboard::BiDashboardRepo;
pub use bi_dashboard::BiDashboardRevisionRepo;
pub use config::ConfigRepo;
pub use dict_entry::DictEntryRepo;
pub use dict_type::DictTypeRepo;
pub use file::FileRepo;
pub use internal_message::InternalMessageRepo;
pub use internal_message_category::InternalMessageCategoryRepo;
pub use internal_message_recipient::InternalMessageRecipientRepo;
pub use language::LanguageRepo;
pub use login_policy::LoginPolicyRepo;
pub use menu::MenuRepo;
pub use monitor_alert::MonitorAlertRepo;
pub use notification_channel::NotificationChannelRepo;
pub use notification_delivery::NotificationDeliveryRepo;
pub use notification_preference::NotificationPreferenceRepo;
pub use notification_rule::NotificationRuleRepo;
pub use notification_template::NotificationTemplateRepo;
pub use org_unit::OrgUnitRepo;
pub use permission::PermissionRepo;
pub use permission_group::PermissionGroupRepo;
pub use plan::PlanRepo;
pub use plan_module::PlanModuleRepo;
pub use plan_quota::PlanQuotaRepo;
pub use position::PositionRepo;
pub use print_template::PrintTemplateRepo;
pub use role::RoleRepo;
pub use script::ScriptRepo;
pub use script_log::ScriptLogRepo;
pub use task::TaskRepo;
pub use tenant::TenantRepo;
pub use user::UserRepo;
