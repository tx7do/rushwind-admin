//! AiUsageLogService — 用量台账（只读）+ 当月汇总。

use std::sync::Arc;

use crate::state::{AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::ai::service::v1::{AiUsageLog, ListAiUsageLogResponse, UsageSummaryResponse};
use proto::proto::pagination::PagingRequest;

fn log_proto(r: crate::data::sys_ai_usage_logs::Model) -> AiUsageLog {
    AiUsageLog {
        id: Some(r.id as u64),
        tenant_id: r.tenant_id.map(|v| v as u64),
        provider_id: r.provider_id.map(|v| v as u64),
        conversation_id: r.conversation_id.map(|v| v as u64),
        user_id: r.user_id.map(|v| v as u64),
        model_name: r.model_name,
        prompt_tokens: r.prompt_tokens,
        completion_tokens: r.completion_tokens,
        total_tokens: r.total_tokens,
        duration_ms: r.duration_ms,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct AiUsageLogService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::AiUsageLogServiceHandlers for AiUsageLogService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiUsageLogResponse, StatusError> {
        let repo = crate::data::repos::AiUsageLogRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiUsageLogResponse {
            items: rows.into_iter().map(log_proto).collect(),
            total,
        })
    }

    async fn get_usage_summary(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<UsageSummaryResponse, StatusError> {
        // quota_configured：配额面随计费里程碑接入；台账先如实回答 tokens/calls。
        let (month_tokens, month_calls) = crate::data::repos::AiUsageLogRepo::new(&self.state.db)
            .month_summary()
            .await?;
        Ok(UsageSummaryResponse {
            month_tokens,
            month_calls,
            quota_configured: false,
            quota_limit: 0,
        })
    }
}
