//! ApiAuditLogService — List/Get over `sys_api_audit_logs`

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::audit::service::v1::{
    ApiAuditLog, GetApiAuditLogRequest, ListApiAuditLogResponse,
};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_api_audit_logs::Model) -> ApiAuditLog {
    ApiAuditLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        user_id: r.user_id.map(|v| v as u64),
        username: r.username,
        ip_address: r.ip_address,
        geo_location: None,
        device_info: None,
        referer: r.referer,
        app_version: r.app_version,
        http_method: r.http_method,
        path: r.path,
        request_uri: r.request_uri,
        api_module: r.api_module,
        api_operation: r.api_operation,
        api_description: r.api_description,
        request_id: r.request_id,
        trace_id: r.trace_id,
        span_id: r.span_id,
        latency_ms: r.latency_ms,
        success: r.success,
        status_code: r.status_code,
        reason: r.reason,
        request_header: r.request_header,
        request_body: r.request_body,
        response: r.response,
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct ApiAuditLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::ApiAuditLogServiceHandlers for ApiAuditLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListApiAuditLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_api(&req).await?;
        Ok(ListApiAuditLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetApiAuditLogRequest,
    ) -> Result<ApiAuditLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::audit::service::v1::get_api_audit_log_request::QueryBy
        );
        let row = crate::data::sys_api_audit_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("api audit log"))?;
        Ok(log_proto(row))
    }
}
