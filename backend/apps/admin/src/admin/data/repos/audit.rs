//! AuditRepo — audit-log repos (login, api,
//! operation, data-access, permission, policy-evaluation): newest-first
//! listing with paging over the six append-only tables.

use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};

use crate::admin::paging as admin_paging;
use crate::proto::pagination::PagingRequest;

use crate::admin::data::{
    sys_api_audit_logs, sys_data_access_audit_logs, sys_login_audit_logs, sys_operation_audit_logs,
    sys_permission_audit_logs, sys_policy_evaluation_logs,
};
use crate::admin::state::StatusError;

pub struct AuditRepo<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> AuditRepo<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }
}

macro_rules! audit_paged {
    ($paged:ident, $entity:ident) => {
        pub async fn $paged(
            &self,
            req: &PagingRequest,
        ) -> Result<(Vec<$entity::Model>, u64), StatusError> {
            admin_paging::fetch_paged(
                self.db,
                $entity::Entity::find().order_by_desc($entity::Column::CreatedAt),
                req,
            )
            .await
        }
    };
}

impl<'a> AuditRepo<'a> {
    audit_paged!(paged_login, sys_login_audit_logs);
    audit_paged!(paged_api, sys_api_audit_logs);
    audit_paged!(paged_operation, sys_operation_audit_logs);
    audit_paged!(paged_data_access, sys_data_access_audit_logs);
    audit_paged!(paged_permission, sys_permission_audit_logs);
    audit_paged!(paged_policy_evaluation, sys_policy_evaluation_logs);
}
