//! LoginPolicyService — //! internal/service/service: tenant login-restriction
//! policies (BLACKLIST/WHITELIST × IP/MAC/REGION/TIME/DEVICE).

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::authentication::service::v1::{
    CreateLoginPolicyRequest, DeleteLoginPolicyRequest, GetLoginPolicyRequest,
    ListLoginPolicyResponse, LoginPolicy, UpdateLoginPolicyRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as BLACKLIST.
fn type_to_str(v: i32) -> String {
    mapping::login_policy_type_str(v)
        .unwrap_or("BLACKLIST")
        .into()
}

fn type_to_proto(s: &str) -> i32 {
    mapping::login_policy_type_of(s).unwrap_or(1)
}

/// Unknown rows read as IP.
fn method_to_str(v: i32) -> String {
    mapping::login_policy_method_str(v).unwrap_or("IP").into()
}

fn method_to_proto(s: &str) -> i32 {
    mapping::login_policy_method_of(s).unwrap_or(1)
}

fn policy_proto(r: crate::data::sys_login_policies::Model) -> LoginPolicy {
    LoginPolicy {
        id: Some(r.id as u64),
        target_id: r.target_id.as_deref().and_then(|v| v.parse().ok()),
        method: r.method.as_deref().map(method_to_proto),
        value: r.value,
        reason: r.reason,
        r#type: r.type_column.as_deref().map(type_to_proto),
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct LoginPolicyService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::LoginPolicyServiceHandlers for LoginPolicyService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListLoginPolicyResponse, StatusError> {
        let repo = crate::data::repos::LoginPolicyRepo::new(
            &self.state.db,
            crate::data::Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListLoginPolicyResponse {
            items: rows.into_iter().map(policy_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetLoginPolicyRequest,
    ) -> Result<LoginPolicy, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::authentication::service::v1::get_login_policy_request::QueryBy
        );
        let row = crate::data::sys_login_policies::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("login policy"))?;
        Ok(policy_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateLoginPolicyRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::sys_login_policies::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            target_id: Set(data.target_id.map(|v| v.to_string())),
            value: Set(data.value),
            reason: Set(data.reason),
            type_column: Set(Some(
                data.r#type
                    .map(type_to_str)
                    .unwrap_or_else(|| "BLACKLIST".into()),
            )),
            method: Set(Some(
                data.method
                    .map(method_to_str)
                    .unwrap_or_else(|| "IP".into()),
            )),
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
        req: UpdateLoginPolicyRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_login_policies::Entity::find_by_id(req.id as i64)
            .filter(crate::data::sys_login_policies::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("login policy"))?;
        let mut a: crate::data::sys_login_policies::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = data.target_id {
                a.target_id = Set(Some(v.to_string()));
            }
            if let Some(v) = &data.value {
                a.value = Set(Some(v.clone()));
            }
            if let Some(v) = &data.reason {
                a.reason = Set(Some(v.clone()));
            }
            if let Some(v) = data.r#type {
                a.type_column = Set(Some(type_to_str(v)));
            }
            if let Some(v) = data.method {
                a.method = Set(Some(method_to_str(v)));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteLoginPolicyRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::authentication::service::v1::delete_login_policy_request::QueryBy
        );
        crate::data::sys_login_policies::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
