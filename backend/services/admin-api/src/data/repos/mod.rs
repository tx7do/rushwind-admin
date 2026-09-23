//! Repository layer. Repos own ALL
//! query predicates: tenancy rides the [`Viewer`], never ad-hoc filters
//! in services.
//!
//! The full repo surface is the data-layer API.

/// The uniform repository shell: connection state, the table predicate,
/// and the two listing envelopes. Repo-specific methods live in the
/// file's own `impl` block beside the invocation.
///
/// * `tenant` arm — the tenancy predicate from the viewer (tenant
///   viewers constrained, platform/system wide);
/// * `global` arm — platform-global tables, no tenant predicate and no
///   viewer state at all.
///
/// Paths resolve at the expansion site — tenant-arm repo files keep
/// importing `Condition`, `ColumnTrait`, `DatabaseConnection`,
/// `EntityTrait`, `QueryFilter`, `Viewer`, `db_err` and `StatusError`;
/// global-arm files need the same set minus `ColumnTrait` and `Viewer`.
macro_rules! repo_shell {
    (tenant $name:ident, $entity:ident) => {
        pub struct $name<'a> {
            pub db: &'a DatabaseConnection,
            pub viewer: Viewer,
        }

        impl<'a> $name<'a> {
            pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
                Self { db, viewer }
            }

            /// The tenancy predicate: tenant viewers are constrained,
            /// platform/system viewers see all rows.
            fn condition(&self) -> Condition {
                match self.viewer.tenant_scope() {
                    Some(tid) => Condition::all().add($entity::Column::TenantId.eq(tid)),
                    None => Condition::all(),
                }
            }

            /// Unpaged listing.
            pub async fn list(&self) -> Result<Vec<$entity::Model>, StatusError> {
                $entity::Entity::find()
                    .filter(self.condition())
                    .all(self.db)
                    .await
                    .map_err(db_err)
            }

            /// Paged listing over the PagingRequest contract: returns (rows, total).
            pub async fn paged_list(
                &self,
                req: &proto::proto::pagination::PagingRequest,
            ) -> Result<(Vec<$entity::Model>, u64), StatusError> {
                crate::paging::fetch_paged(
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
            pub db: &'a DatabaseConnection,
        }

        impl<'a> $name<'a> {
            pub fn new(db: &'a DatabaseConnection) -> Self {
                Self { db }
            }

            // Platform-global table: no tenant predicate applies.
            fn condition(&self) -> Condition {
                Condition::all()
            }

            /// Unpaged listing.
            pub async fn list(&self) -> Result<Vec<$entity::Model>, StatusError> {
                $entity::Entity::find()
                    .filter(self.condition())
                    .all(self.db)
                    .await
                    .map_err(db_err)
            }

            /// Paged listing over the PagingRequest contract: returns (rows, total).
            pub async fn paged_list(
                &self,
                req: &proto::proto::pagination::PagingRequest,
            ) -> Result<(Vec<$entity::Model>, u64), StatusError> {
                crate::paging::fetch_paged(
                    self.db,
                    $entity::Entity::find().filter(self.condition()),
                    req,
                )
                .await
            }
        }
    };
}
// Textual scope: every repo module below sees the macro (same pattern as
// the audit-suite macros inside their file).

mod access_key;
mod api;
mod audit;
mod config;
mod dict;
mod file;
mod language;
mod login_policy;
mod menu;
mod message;
mod notification_channel;
mod notification_delivery;
mod notification_rule;
mod org_unit;
mod permission;
mod permission_group;
mod plan;
mod position;
mod role;
mod script;
mod task;
mod tenant;
mod user;

pub use access_key::AccessKeyRepo;
pub use api::ApiRepo;
pub use audit::AuditRepo;
pub use config::ConfigRepo;
pub use dict::DictEntryRepo;
pub use dict::DictTypeRepo;
pub use file::FileRepo;
pub use language::LanguageRepo;
pub use login_policy::LoginPolicyRepo;
pub use menu::MenuRepo;
pub use message::InternalMessageCategoryRepo;
pub use message::InternalMessageRecipientRepo;
pub use message::InternalMessageRepo;
pub use notification_channel::NotificationChannelRepo;
pub use notification_delivery::NotificationDeliveryRepo;
pub use notification_rule::NotificationRuleRepo;
pub use org_unit::OrgUnitRepo;
pub use permission::PermissionRepo;
pub use permission_group::PermissionGroupRepo;
pub use plan::PlanModuleRepo;
pub use plan::PlanQuotaRepo;
pub use plan::PlanRepo;
pub use position::PositionRepo;
pub use role::RoleRepo;
pub use script::ScriptLogRepo;
pub use script::ScriptRepo;
pub use task::TaskRepo;
pub use tenant::TenantRepo;
pub use user::UserRepo;
