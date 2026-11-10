//! PermissionAuditLogService — List/Get over `sys_permission_audit_logs`

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::mapping;
use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::audit::service::v1::{
    GetPermissionAuditLogRequest, ListPermissionAuditLogResponse, PermissionAuditLog,
};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_permission_audit_logs::Model) -> PermissionAuditLog {
    PermissionAuditLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        operator_id: r.operator_id.map(|v| v as u64),
        operator_name: r.operator_name,
        target_type: r.target_type,
        target_id: r.target_id,
        target_name: r.target_name,
        action: r
            .action
            .as_deref()
            .map(|s| mapping::permission_audit_action_of(s).unwrap_or(14)),
        old_value: r.old_value.as_ref().map(|v| v.to_string()),
        new_value: r.new_value.as_ref().map(|v| v.to_string()),
        ip_address: r.ip_address,
        request_id: r.request_id,
        reason: r.reason,
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct PermissionAuditLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PermissionAuditLogServiceHandlers for PermissionAuditLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPermissionAuditLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_permission(&req).await?;
        Ok(ListPermissionAuditLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPermissionAuditLogRequest,
    ) -> Result<PermissionAuditLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::audit::service::v1::get_permission_audit_log_request::QueryBy
        );
        let row = crate::data::sys_permission_audit_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("permission audit log"))?;
        Ok(log_proto(row))
    }
}
