//! PositionService — //! internal/service/service: position CRUD over
//! `sys_positions`.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::{
    CreatePositionRequest, DeletePositionRequest, GetPositionRequest, ListPositionResponse,
    Position, UpdatePositionRequest,
};
use proto::proto::pagination::PagingRequest;

fn position_proto(r: crate::data::sys_positions::Model) -> Position {
    Position {
        id: Some(r.id as u64),
        name: Some(r.name),
        code: r.code,
        headcount: r.headcount,
        sort_order: r.sort_order,
        status: r.status.as_deref().map(crate::state::status_to_proto),
        r#type: r
            .type_column
            .as_deref()
            .map(|s| mapping::position_type_of(s).unwrap_or(0)),
        remark: r.remark,
        description: r.description,
        job_family: r.job_family,
        job_grade: r.job_grade,
        level: r.level,
        is_key_position: r.is_key_position,
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        org_unit_id: r.org_unit_id.map(|v| v as u64),
        org_unit_name: None,
        reports_to_position_id: r.reports_to_position_id.map(|v| v as u64),
        reports_to_position_name: None,
        start_at: r.start_at.and_then(crate::state::naive_to_ts),
        end_at: r.end_at.and_then(crate::state::naive_to_ts),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct PositionService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PositionServiceHandlers for PositionService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPositionResponse, StatusError> {
        let repo = crate::data::repos::PositionRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPositionResponse {
            items: rows.into_iter().map(position_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPositionRequest,
    ) -> Result<Position, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::get_position_request::QueryBy
        );
        let row = crate::data::sys_positions::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("position"))?;
        Ok(position_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePositionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_positions::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.unwrap_or_default()),
            code: Set(data.code),
            org_unit_id: Set(data.org_unit_id.map(|v| v as i64)),
            reports_to_position_id: Set(data.reports_to_position_id.map(|v| v as i64)),
            description: Set(data.description),
            job_family: Set(data.job_family),
            job_grade: Set(data.job_grade),
            level: Set(data.level),
            headcount: Set(data.headcount.or(Some(0))),
            is_key_position: Set(data.is_key_position.or(Some(false))),
            type_column: Set(Some(match data.r#type.unwrap_or(0) {
                1 => "MANAGER".to_string(),
                2 => "LEAD".to_string(),
                3 => "INTERN".to_string(),
                4 => "CONTRACT".to_string(),
                5 => "OTHER".to_string(),
                _ => "REGULAR".to_string(),
            })),
            status: Set(Some("ON".into())),
            sort_order: Set(data.sort_order.or(Some(0))),
            remark: Set(data.remark),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdatePositionRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_positions::Entity::find_by_id(req.id as i64)
            .filter(crate::data::sys_positions::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("position"))?;
        let mut a: crate::data::sys_positions::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.headcount {
                a.headcount = Set(Some(v));
            }
            if let Some(v) = data.sort_order {
                a.sort_order = Set(Some(v));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(if v == 0 { "OFF".into() } else { "ON".into() }));
            }
            if let Some(v) = data.org_unit_id {
                a.org_unit_id = Set(Some(v as i64));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePositionRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_position_request::QueryBy
        );
        crate::data::sys_positions::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
