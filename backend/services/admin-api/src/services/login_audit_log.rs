//! LoginAuditLogService — List/Get over `sys_login_audit_logs`

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::mapping;
use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::audit::service::v1::{
    GetLoginAuditLogRequest, ListLoginAuditLogResponse, LoginAuditLog,
};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_login_audit_logs::Model) -> LoginAuditLog {
    LoginAuditLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        user_id: r.user_id.map(|v| v as u64),
        username: r.username,
        ip_address: r.ip_address,
        geo_location: None,
        session_id: r.session_id,
        device_info: None,
        request_id: r.request_id,
        trace_id: r.trace_id,
        action_type: r
            .action_type
            .as_deref()
            .map(|s| mapping::login_audit_action_of(s).unwrap_or(0)),
        status: r
            .status
            .as_deref()
            .map(|s| mapping::login_audit_status_of(s).unwrap_or(0)),
        failure_reason: r.failure_reason,
        mfa_status: r.mfa_status,
        login_method: r
            .login_method
            .as_deref()
            .map(|s| mapping::login_audit_method_of(s).unwrap_or(0)),
        risk_score: r.risk_score,
        risk_level: r
            .risk_level
            .as_deref()
            .map(|s| mapping::login_audit_risk_level_of(s).unwrap_or(0)),
        risk_factors: r
            .risk_factors
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct LoginAuditLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::LoginAuditLogServiceHandlers for LoginAuditLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListLoginAuditLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_login(&req).await?;
        Ok(ListLoginAuditLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetLoginAuditLogRequest,
    ) -> Result<LoginAuditLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::audit::service::v1::get_login_audit_log_request::QueryBy
        );
        let row = crate::data::sys_login_audit_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("login audit log"))?;
        Ok(log_proto(row))
    }
}
