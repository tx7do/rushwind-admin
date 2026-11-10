//! DataAccessAuditLogService — List/Get over `sys_data_access_audit_logs`

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::mapping;
use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::audit::service::v1::{
    DataAccessAuditLog, GetDataAccessAuditLogRequest, ListDataAccessAuditLogResponse,
};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_data_access_audit_logs::Model) -> DataAccessAuditLog {
    DataAccessAuditLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        user_id: r.user_id.map(|v| v as u64),
        username: r.username,
        ip_address: r.ip_address,
        request_id: r.request_id,
        data_source: r.data_source,
        table_name: r.table_name,
        data_id: r.data_id,
        access_type: r
            .access_type
            .as_deref()
            .map(|s| mapping::data_access_type_of(s).unwrap_or(0)),
        sql_digest: r.sql_digest,
        sql_text: r.sql_text,
        affected_rows: r.affected_rows,
        latency_ms: r.latency_ms,
        success: r.success,
        sensitive_level: r
            .sensitive_level
            .as_deref()
            .map(|s| mapping::data_access_sensitive_level_of(s).unwrap_or(0)),
        data_masked: r.data_masked,
        masking_rules: r.masking_rules,
        business_purpose: r.business_purpose,
        data_category: r.data_category,
        db_user: r.db_user,
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct DataAccessAuditLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::DataAccessAuditLogServiceHandlers for DataAccessAuditLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListDataAccessAuditLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_data_access(&req).await?;
        Ok(ListDataAccessAuditLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetDataAccessAuditLogRequest,
    ) -> Result<DataAccessAuditLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::audit::service::v1::get_data_access_audit_log_request::QueryBy
        );
        let row = crate::data::sys_data_access_audit_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("data access audit log"))?;
        Ok(log_proto(row))
    }
}
