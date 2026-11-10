//! OperationAuditLogService — List/Get over `sys_operation_audit_logs`

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::mapping;
use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::audit::service::v1::{
    GetOperationAuditLogRequest, ListOperationAuditLogResponse, OperationAuditLog,
};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_operation_audit_logs::Model) -> OperationAuditLog {
    OperationAuditLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        user_id: r.user_id.map(|v| v as u64),
        username: r.username,
        resource_type: r.resource_type,
        resource_id: r.resource_id,
        action: r
            .action
            .as_deref()
            .map(|s| mapping::operation_audit_action_of(s).unwrap_or(8)),
        before_data: r.before_data.as_ref().map(|v| v.to_string()),
        after_data: r.after_data.as_ref().map(|v| v.to_string()),
        sensitive_level: r
            .sensitive_level
            .as_deref()
            .map(|s| mapping::operation_audit_sensitive_level_of(s).unwrap_or(3)),
        request_id: r.request_id,
        trace_id: r.trace_id,
        success: r.success,
        failure_reason: r.failure_reason,
        ip_address: r.ip_address,
        geo_location: None,
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct OperationAuditLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::OperationAuditLogServiceHandlers for OperationAuditLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListOperationAuditLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_operation(&req).await?;
        Ok(ListOperationAuditLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetOperationAuditLogRequest,
    ) -> Result<OperationAuditLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::audit::service::v1::get_operation_audit_log_request::QueryBy
        );
        let row = crate::data::sys_operation_audit_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("operation audit log"))?;
        Ok(log_proto(row))
    }
}
