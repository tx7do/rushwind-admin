//! Plan / PlanModule / PlanQuota services — //! module, module and service:
//! subscription-plan CRUD with their module-whitelist and quota children.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::mapping;
use crate::admin::state::{db_err, not_found, operator_of, AppState, StatusError};
use crate::proto::identity::service::v1::{
    CreatePlanModuleRequest, CreatePlanQuotaRequest, CreatePlanRequest, DeletePlanModuleRequest,
    DeletePlanQuotaRequest, DeletePlanRequest, GetPlanModuleRequest, GetPlanRequest,
    ListPlanModuleResponse, ListPlanQuotaResponse, ListPlanResponse, Plan, PlanModule, PlanQuota,
    UpdatePlanModuleRequest, UpdatePlanQuotaRequest, UpdatePlanRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

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

fn plan_proto(r: crate::admin::data::sys_plans::Model) -> Plan {
    Plan {
        id: Some(r.id),
        name: Some(r.name),
        version: r.version.as_deref().map(plan_version_to_proto),
        expiry_policy: r.expiry_policy.as_deref().map(expiry_policy_to_proto),
        data_retention_days: r.data_retention_days,
        description: r.description,
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

fn plan_module_proto(r: crate::admin::data::sys_plan_modules::Model) -> PlanModule {
    PlanModule {
        id: Some(r.id),
        plan_id: Some(r.plan_id),
        module: r
            .module
            .as_deref()
            .map(|s| mapping::menu_module_of(s).unwrap_or(0)),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

fn plan_quota_proto(r: crate::admin::data::sys_plan_quotas::Model) -> PlanQuota {
    PlanQuota {
        id: Some(r.id),
        plan_id: Some(r.plan_id),
        quota_type: r
            .quota_type
            .as_deref()
            .map(|s| mapping::plan_quota_type_of(s).unwrap_or(0)),
        quota_value: r.quota_value.map(|v| v as u64),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::datetime_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::datetime_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::datetime_to_ts),
    }
}

pub struct PlanService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::PlanServiceHandlers for PlanService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanResponse, StatusError> {
        let repo = crate::admin::data::repos::PlanRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanResponse {
            items: rows.into_iter().map(plan_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPlanRequest,
    ) -> Result<Plan, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::get_plan_request::QueryBy
        );
        let row = crate::admin::data::sys_plans::Entity::find_by_id(id)
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
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_plans::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            version: Set(Some(plan_version_to_str(data.version.unwrap_or(0)))),
            expiry_policy: Set(Some(expiry_policy_to_str(data.expiry_policy.unwrap_or(0)))),
            data_retention_days: Set(data.data_retention_days),
            description: Set(data.description),
            remark: Set(data.remark),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
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
        let row = crate::admin::data::sys_plans::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan"))?;
        let mut a: crate::admin::data::sys_plans::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
            crate::set_masked!(opt mask, a, version, data.version, |v: &i32| Some(
                plan_version_to_str(*v)
            ));
            crate::set_masked!(opt mask, a, expiry_policy, data.expiry_policy, |v: &i32| Some(
                expiry_policy_to_str(*v)
            ));
            crate::set_masked!(opt mask, a, data_retention_days, data.data_retention_days, |v: &u32| Some(
                *v
            ));
            crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
                v.clone()
            ));
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
            crate::proto::identity::service::v1::delete_plan_request::QueryBy
        );
        // CASCADE children.
        crate::admin::data::sys_plan_modules::Entity::delete_many()
            .filter(crate::admin::data::sys_plan_modules::Column::PlanId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_plan_quotas::Entity::delete_many()
            .filter(crate::admin::data::sys_plan_quotas::Column::PlanId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_plans::Entity::delete_by_id(id)
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
impl crate::proto::gen::services::PlanModuleServiceHandlers for PlanModuleService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanModuleResponse, StatusError> {
        let repo = crate::admin::data::repos::PlanModuleRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanModuleResponse {
            items: rows.into_iter().map(plan_module_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPlanModuleRequest,
    ) -> Result<PlanModule, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::get_plan_module_request::QueryBy
        );
        let row = crate::admin::data::sys_plan_modules::Entity::find_by_id(id)
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
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_plan_modules::ActiveModel {
            plan_id: Set(data.plan_id.unwrap_or(0)),
            module: Set(Some(module_to_str(data.module.unwrap_or(0)))),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
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
        let row = crate::admin::data::sys_plan_modules::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan module"))?;
        let mut a: crate::admin::data::sys_plan_modules::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, a, module, data.module, |v: &i32| Some(
                module_to_str(*v)
            ));
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
            crate::proto::identity::service::v1::delete_plan_module_request::QueryBy
        );
        crate::admin::data::sys_plan_modules::Entity::delete_by_id(id)
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
impl crate::proto::gen::services::PlanQuotaServiceHandlers for PlanQuotaService {
    async fn list(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPlanQuotaResponse, StatusError> {
        let repo = crate::admin::data::repos::PlanQuotaRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPlanQuotaResponse {
            items: rows.into_iter().map(plan_quota_proto).collect(),
            total: total as i64,
        })
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePlanQuotaRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        crate::admin::data::sys_plan_quotas::ActiveModel {
            plan_id: Set(data.plan_id.unwrap_or(0)),
            quota_type: Set(Some(quota_type_to_str(data.quota_type.unwrap_or(0)))),
            quota_value: Set(data.quota_value.map(|v| v as i64)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
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
        let row = crate::admin::data::sys_plan_quotas::Entity::find_by_id(req.id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("plan quota"))?;
        let mut a: crate::admin::data::sys_plan_quotas::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, a, quota_value, data.quota_value, |v: &u64| Some(
                *v as i64
            ));
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
            crate::proto::identity::service::v1::delete_plan_quota_request::QueryBy
        );
        crate::admin::data::sys_plan_quotas::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{sys_plan_modules, sys_plan_quotas, sys_plans};
    use crate::proto::gen::services::{
        PlanModuleServiceHandlers as _, PlanQuotaServiceHandlers as _, PlanServiceHandlers as _,
    };
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant_plan(
        db: &sea_orm::DatabaseConnection,
        name: &str,
        remark: &str,
    ) -> sys_plans::Model {
        sys_plans::ActiveModel {
            name: Set(name.into()),
            version: Set(Some("STANDARD".into())),
            expiry_policy: Set(Some("READONLY".into())),
            data_retention_days: Set(Some(30)),
            description: Set(Some("旧描述".into())),
            remark: Set(Some(remark.into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_module(
        db: &sea_orm::DatabaseConnection,
        plan_id: i64,
        module: &str,
    ) -> sys_plan_modules::Model {
        sys_plan_modules::ActiveModel {
            plan_id: Set(plan_id),
            module: Set(Some(module.into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_quota(
        db: &sea_orm::DatabaseConnection,
        plan_id: i64,
        quota_value: i64,
    ) -> sys_plan_quotas::Model {
        sys_plan_quotas::ActiveModel {
            plan_id: Set(plan_id),
            quota_type: Set(Some("USER_LIMIT".into())),
            quota_value: Set(Some(quota_value)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plan_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> sys_plans::Model {
        sys_plans::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn module_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> sys_plan_modules::Model {
        sys_plan_modules::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn quota_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> sys_plan_quotas::Model {
        sys_plan_quotas::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn field_mask(paths: &[&str]) -> Option<pbjson_types::FieldMask> {
        Some(pbjson_types::FieldMask {
            paths: paths.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// update_mask 白名单门（plan）：掩码只选 name 时，载荷里多带的
    /// remark 与 data_retention_days 不许落列。
    #[tokio::test]
    async fn plan_mask_gates_unlisted_fields() {
        let state = test_state().await;
        let svc = PlanService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant_plan(db, "基础版", "旧备注").await;

        svc.update(
            me,
            UpdatePlanRequest {
                id: row.id,
                data: Some(Plan {
                    name: Some("专业版".into()),
                    remark: Some("越权备注".into()),
                    data_retention_days: Some(99),
                    ..Default::default()
                }),
                update_mask: field_mask(&["name"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();

        let fresh = plan_by_id(db, row.id).await;
        assert_eq!(fresh.name, "专业版", "masked field lands");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧备注"),
            "unmasked remark is not writable through the gate"
        );
        assert_eq!(
            fresh.data_retention_days,
            Some(30),
            "unmasked data_retention_days is not writable through the gate"
        );
    }

    /// plan 掩码选中 + 载荷 None = 显式清列（可空列清值通道）；NOT NULL
    /// 列（name）同样选中时跳过不清；无掩码 None 仍是"不动"。
    #[tokio::test]
    async fn plan_masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = PlanService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant_plan(db, "基础版", "待清备注").await;

        // 无掩码：None 不动列（全量提交语义不回退）。
        svc.update(
            ctx(0, 1),
            UpdatePlanRequest {
                id: row.id,
                data: Some(Plan {
                    description: Some("新描述".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = plan_by_id(db, row.id).await;
        assert_eq!(fresh.description.as_deref(), Some("新描述"));
        assert_eq!(
            fresh.remark.as_deref(),
            Some("待清备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 name + remark、载荷全 None：可空 remark 清列，NOT NULL
        // name 原样保留。
        svc.update(
            ctx(0, 1),
            UpdatePlanRequest {
                id: row.id,
                data: Some(Plan::default()),
                update_mask: field_mask(&["name", "remark"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = plan_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(fresh.name, "基础版", "NOT NULL column never clears");
    }

    /// update_mask 白名单门（plan_module）：掩码只选 plan_id 时，载荷里
    /// 携带的 module 不许落列。
    #[tokio::test]
    async fn plan_module_mask_gates_unlisted_fields() {
        let state = test_state().await;
        let svc = PlanModuleService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant_module(db, 1, "DASHBOARD").await;

        svc.update(
            me,
            UpdatePlanModuleRequest {
                id: row.id,
                data: Some(PlanModule {
                    plan_id: Some(99),
                    module: Some(5),
                    ..Default::default()
                }),
                update_mask: field_mask(&["plan_id"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();

        let fresh = module_by_id(db, row.id).await;
        assert_eq!(
            fresh.module.as_deref(),
            Some("DASHBOARD"),
            "unmasked module is not writable through the gate"
        );
        assert_eq!(
            fresh.plan_id, 1,
            "plan_id stays (not an update-writable column)"
        );
    }

    /// plan_module 掩码选中 + 载荷 None = 显式清列；无掩码 None 仍是
    /// "不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn plan_module_masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = PlanModuleService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant_module(db, 1, "DASHBOARD").await;

        // 无掩码：None 不动列。
        svc.update(
            ctx(0, 1),
            UpdatePlanModuleRequest {
                id: row.id,
                data: Some(PlanModule::default()),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = module_by_id(db, row.id).await;
        assert_eq!(
            fresh.module.as_deref(),
            Some("DASHBOARD"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 module、载荷 None：可空 module 清列。
        svc.update(
            ctx(0, 1),
            UpdatePlanModuleRequest {
                id: row.id,
                data: Some(PlanModule::default()),
                update_mask: field_mask(&["module"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = module_by_id(db, row.id).await;
        assert!(
            fresh.module.is_none(),
            "masked None explicitly clears the column"
        );
    }

    /// update_mask 白名单门（plan_quota）：掩码只选 quota_type 时，载荷里
    /// 携带的 quota_value 不许落列。
    #[tokio::test]
    async fn plan_quota_mask_gates_unlisted_fields() {
        let state = test_state().await;
        let svc = PlanQuotaService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant_quota(db, 1, 100).await;

        svc.update(
            me,
            UpdatePlanQuotaRequest {
                id: row.id,
                data: Some(PlanQuota {
                    quota_value: Some(500),
                    ..Default::default()
                }),
                update_mask: field_mask(&["quota_type"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();

        let fresh = quota_by_id(db, row.id).await;
        assert_eq!(
            fresh.quota_value,
            Some(100),
            "unmasked quota_value is not writable through the gate"
        );
    }

    /// plan_quota 掩码选中 + 载荷 None = 显式清列；无掩码 None 仍是
    /// "不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn plan_quota_masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = PlanQuotaService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant_quota(db, 1, 100).await;

        // 无掩码：None 不动列。
        svc.update(
            ctx(0, 1),
            UpdatePlanQuotaRequest {
                id: row.id,
                data: Some(PlanQuota::default()),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = quota_by_id(db, row.id).await;
        assert_eq!(
            fresh.quota_value,
            Some(100),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 quota_value、载荷 None：可空 quota_value 清列。
        svc.update(
            ctx(0, 1),
            UpdatePlanQuotaRequest {
                id: row.id,
                data: Some(PlanQuota::default()),
                update_mask: field_mask(&["quota_value"]),
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = quota_by_id(db, row.id).await;
        assert!(
            fresh.quota_value.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
