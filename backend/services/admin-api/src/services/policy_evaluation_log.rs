//! PolicyEvaluationLogService — List/Get over
//! `sys_policy_evaluation_logs` (
//! module; lives in the permission module).

use std::sync::Arc;

use sea_orm::EntityTrait;

use crate::state::{db_err, not_found, AppState, StatusError};
use proto::proto::pagination::PagingRequest;
use proto::proto::permission::service::v1::{
    GetPolicyEvaluationLogRequest, ListPolicyEvaluationLogResponse, PolicyEvaluationLog,
};

fn log_proto(r: crate::data::sys_policy_evaluation_logs::Model) -> PolicyEvaluationLog {
    PolicyEvaluationLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        user_id: r.user_id.map(|v| v as u64),
        membership_id: r.membership_id.map(|v| v as u64),
        permission_id: r.permission_id.map(|v| v as u64),
        policy_id: r.policy_id.map(|v| v as u64),
        request_path: r.request_path,
        request_method: r.request_method,
        result: r.result,
        effect_details: r.effect_details,
        scope_sql: r.scope_sql,
        ip_address: r.ip_address,
        trace_id: r.trace_id,
        evaluation_context: r.evaluation_context,
        log_hash: r.log_hash,
        signature: r.signature,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct PolicyEvaluationLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PolicyEvaluationLogServiceHandlers for PolicyEvaluationLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPolicyEvaluationLogResponse, StatusError> {
        let repo = crate::data::repos::AuditRepo::new(&self.state.db);
        let (rows, total) = repo.paged_policy_evaluation(&req).await?;
        Ok(ListPolicyEvaluationLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPolicyEvaluationLogRequest,
    ) -> Result<PolicyEvaluationLog, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::permission::service::v1::get_policy_evaluation_log_request::QueryBy
        );
        let row = crate::data::sys_policy_evaluation_logs::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("policy evaluation log"))?;
        Ok(log_proto(row))
    }
}
