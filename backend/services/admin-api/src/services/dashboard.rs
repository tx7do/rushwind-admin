//! DashboardService — //! internal/service/service: overview counts, login trend,
//! action/status distributions from the audit tables.

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use crate::state::{db_err, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::admin::service::v1::{
    ActionDistributionResponse, DashboardOverviewResponse, DistributionItem, GetLoginTrendRequest,
    LoginTrendResponse, StatusDistributionResponse, TrendPoint,
};

pub struct DashboardService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::DashboardServiceHandlers for DashboardService {
    async fn get_overview(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<DashboardOverviewResponse, StatusError> {
        let user_count = crate::data::sys_users::Entity::find()
            .count(&self.state.db)
            .await
            .unwrap_or(0) as u32;
        let role_count = crate::data::sys_roles::Entity::find()
            .count(&self.state.db)
            .await
            .unwrap_or(0) as u32;
        let today = crate::data::now().date();
        let today_login_count = crate::data::sys_login_audit_logs::Entity::find()
            .filter(
                crate::data::sys_login_audit_logs::Column::CreatedAt
                    .gte(today.and_hms_opt(0, 0, 0)),
            )
            .count(&self.state.db)
            .await
            .unwrap_or(0) as u32;
        let today_operation_count = crate::data::sys_operation_audit_logs::Entity::find()
            .filter(
                crate::data::sys_operation_audit_logs::Column::CreatedAt
                    .gte(today.and_hms_opt(0, 0, 0)),
            )
            .count(&self.state.db)
            .await
            .unwrap_or(0) as u32;
        Ok(DashboardOverviewResponse {
            user_count,
            role_count,
            today_login_count,
            today_operation_count,
        })
    }

    async fn get_login_trend(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetLoginTrendRequest,
    ) -> Result<LoginTrendResponse, StatusError> {
        let days = req.days.unwrap_or(7).clamp(1, 90) as i64;
        let rows = crate::data::sys_login_audit_logs::Entity::find()
            .filter(
                crate::data::sys_login_audit_logs::Column::CreatedAt
                    .gte(crate::data::now().date() - chrono::Duration::days(days)),
            )
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut buckets: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for offset in (0..days).rev() {
            let date = (crate::data::now().date() - chrono::Duration::days(offset)).to_string();
            buckets.insert(date, 0);
        }
        for row in rows {
            if let Some(created) = row.created_at {
                let key = created.date().to_string();
                if let Some(count) = buckets.get_mut(&key) {
                    *count += 1;
                }
            }
        }
        Ok(LoginTrendResponse {
            points: buckets
                .into_iter()
                .map(|(date, count)| TrendPoint { date, count })
                .collect(),
        })
    }

    async fn get_operation_action_distribution(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ActionDistributionResponse, StatusError> {
        let rows = crate::data::sys_operation_audit_logs::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for row in rows {
            let action = row.action.unwrap_or_default();
            let label = row.resource_type.clone().unwrap_or_else(|| "other".into());
            let _ = action;
            *counts.entry(label).or_insert(0) += 1;
        }
        Ok(ActionDistributionResponse {
            items: counts
                .into_iter()
                .map(|(label, count)| DistributionItem { label, count })
                .collect(),
        })
    }

    async fn get_login_status_distribution(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<StatusDistributionResponse, StatusError> {
        let rows = crate::data::sys_login_audit_logs::Entity::find()
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for row in rows {
            let label = if row.status.as_deref() == Some("SUCCESS") {
                "success"
            } else {
                "failed"
            }
            .to_string();
            *counts.entry(label).or_insert(0) += 1;
        }
        Ok(StatusDistributionResponse {
            items: counts
                .into_iter()
                .map(|(label, count)| DistributionItem { label, count })
                .collect(),
        })
    }

    async fn get_ai_insights(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: proto::proto::admin::service::v1::AiInsightsRequest,
    ) -> Result<proto::proto::admin::service::v1::AiInsightsResponse, StatusError> {
        // The LLM-backed dashboard insights ride the AI domain increment;
        // the panel reads empty until then.
        Ok(proto::proto::admin::service::v1::AiInsightsResponse {
            alerts: Vec::new(),
            summary: String::new(),
        })
    }
}
