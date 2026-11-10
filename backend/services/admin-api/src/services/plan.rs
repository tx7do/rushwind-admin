//! Plan / PlanModule / PlanQuota services — //! module, module and service:
//! subscription-plan CRUD with their module-whitelist and quota children.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::{
    CreatePlanModuleRequest, CreatePlanQuotaRequest, CreatePlanRequest, DeletePlanModuleRequest,
    DeletePlanQuotaRequest, DeletePlanRequest, GetPlanModuleRequest, GetPlanRequest,
    ListPlanModuleResponse, ListPlanQuotaResponse, ListPlanResponse, Plan, PlanModule, PlanQuota,
    UpdatePlanModuleRequest, UpdatePlanQuotaRequest, UpdatePlanRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as FREE.
fn plan_version_to_str(v: i32) -> String {
    mapping::plan_version_str(v).unwrap_or("FREE").into()
}

fn plan_version_to_proto(s: &str) -> i32 {
    mapping::plan_version_of(s).unwrap_or(0)
}

/// Unknown rows read as READONLY.
fn expiry_policy_to_proto(s: &str) -> i32 {
    mapping::plan_expiry_policy_of(s).unwrap_or(0)
}

fn expiry_policy_to_str(v: i32) -> String {
    mapping::plan_expiry_policy_str(v)
        .unwrap_or("READONLY")
        .into()
}

/// Unknown rows read as SYSTEM.
fn module_to_str(v: i32) -> String {
    mapping::menu_module_str(v).unwrap_or("SYSTEM").into()
}

/// Unknown rows read as USER_LIMIT.
fn quota_type_to_str(v: i32) -> String {
    mapping::plan_quota_type_str(v)
        .unwrap_or("USER_LIMIT")
        .into()
}

fn plan_proto(r: crate::data::sys_plans::Model) -> Plan {
    Plan {
        id: Some(r.id as u64),
        name: Some(r.name),
        version: r.version.as_deref().map(plan_version_to_proto),
        expiry_policy: r.expiry_policy.as_deref().map(expiry_policy_to_proto),
        data_retention_days: r.data_retention_days,
        description: r.description,
        remark: r.remark,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn plan_module_proto(r: crate::data::sys_plan_modules::Model) -> PlanModule {
    PlanModule {
        id: Some(r.id as u64),
        plan_id: Some(r.plan_id as u64),
        module: r
            .module
            .as_deref()
            .map(|s| mapping::menu_module_of(s).unwrap_or(0)),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn plan_quota_proto(r: crate::data::sys_plan_quotas::Model) -> PlanQuota {
    PlanQuota {
        id: Some(r.id as u64),
        plan_id: Some(r.plan_id as u64),
        quota_type: r
            .quota_type
            .as_deref()
            .map(|s| mapping::plan_quota_type_of(s).unwrap_or(0)),
        quota_value: r.quota_value.map(|v| v as u64),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct PlanService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PlanServiceHandlers for PlanService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanResponse, StatusError> {
        let repo = crate::data::repos::PlanRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanResponse {
            items: rows.into_iter().map(plan_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPlanRequest,
    ) -> Result<Plan, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::get_plan_request::QueryBy
        );
        let row = crate::data::sys_plans::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan"))?;
        Ok(plan_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePlanRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_plans::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            version: Set(Some(plan_version_to_str(data.version.unwrap_or(0)))),
            expiry_policy: Set(Some(expiry_policy_to_str(data.expiry_policy.unwrap_or(0)))),
            data_retention_days: Set(data.data_retention_days),
            description: Set(data.description),
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
        req: UpdatePlanRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_plans::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan"))?;
        let mut a: crate::data::sys_plans::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = data.version {
                a.version = Set(Some(plan_version_to_str(v)));
            }
            if let Some(v) = data.expiry_policy {
                a.expiry_policy = Set(Some(expiry_policy_to_str(v)));
            }
            if let Some(v) = data.data_retention_days {
                a.data_retention_days = Set(Some(v));
            }
            if let Some(v) = &data.description {
                a.description = Set(Some(v.clone()));
            }
            if let Some(v) = &data.remark {
                a.remark = Set(Some(v.clone()));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePlanRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_plan_request::QueryBy
        );
        // CASCADE children.
        crate::data::sys_plan_modules::Entity::delete_many()
            .filter(crate::data::sys_plan_modules::Column::PlanId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_plan_quotas::Entity::delete_many()
            .filter(crate::data::sys_plan_quotas::Column::PlanId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_plans::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

pub struct PlanModuleService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PlanModuleServiceHandlers for PlanModuleService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanModuleResponse, StatusError> {
        let repo = crate::data::repos::PlanModuleRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanModuleResponse {
            items: rows.into_iter().map(plan_module_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPlanModuleRequest,
    ) -> Result<PlanModule, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::get_plan_module_request::QueryBy
        );
        let row = crate::data::sys_plan_modules::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan module"))?;
        Ok(plan_module_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePlanModuleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_plan_modules::ActiveModel {
            plan_id: Set(data.plan_id.map(|v| v as i64).unwrap_or(0)),
            module: Set(Some(module_to_str(data.module.unwrap_or(0)))),
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
        req: UpdatePlanModuleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_plan_modules::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan module"))?;
        let mut a: crate::data::sys_plan_modules::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.module {
                a.module = Set(Some(module_to_str(v)));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePlanModuleRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_plan_module_request::QueryBy
        );
        crate::data::sys_plan_modules::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

pub struct PlanQuotaService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PlanQuotaServiceHandlers for PlanQuotaService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanQuotaResponse, StatusError> {
        let repo = crate::data::repos::PlanQuotaRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanQuotaResponse {
            items: rows.into_iter().map(plan_quota_proto).collect(),
            total,
        })
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePlanQuotaRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_plan_quotas::ActiveModel {
            plan_id: Set(data.plan_id.map(|v| v as i64).unwrap_or(0)),
            quota_type: Set(Some(quota_type_to_str(data.quota_type.unwrap_or(0)))),
            quota_value: Set(data.quota_value.map(|v| v as i64)),
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
        req: UpdatePlanQuotaRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_plan_quotas::Entity::find_by_id(req.id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan quota"))?;
        let mut a: crate::data::sys_plan_quotas::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.quota_value {
                a.quota_value = Set(Some(v as i64));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePlanQuotaRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_plan_quota_request::QueryBy
        );
        crate::data::sys_plan_quotas::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
