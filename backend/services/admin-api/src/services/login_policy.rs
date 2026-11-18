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
            // 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即
            // 存量 if-let-Some 全量提交语义不变）。type 的列名是 type_column
            // （wire 名 ≠ 列名）走 opt_named；掩码选中 + 载荷 None = 显式清
            // 可空列；无掩码 None 不动列。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, a, target_id, data.target_id, |v: &u64| Some(
                v.to_string()
            ));
            crate::set_masked!(opt mask, a, value, data.value, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, reason, data.reason, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt_named mask, a, type_column, "type", data.r#type, |v: &i32| Some(
                type_to_str(*v)
            ));
            crate::set_masked!(opt mask, a, method, data.method, |v: &i32| Some(
                method_to_str(*v)
            ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_login_policies as policies;
    use crate::testsupport::{ctx, test_state};
    use proto::gen::services::LoginPolicyServiceHandlers as _;
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, tenant: i64, value: &str) -> policies::Model {
        policies::ActiveModel {
            tenant_id: Set(Some(tenant)),
            target_id: Set(Some("42".into())),
            value: Set(Some(value.into())),
            reason: Set(Some("原始原因".into())),
            type_column: Set(Some("BLACKLIST".into())),
            method: Set(Some("IP".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> policies::Model {
        policies::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: u64, data: LoginPolicy, paths: &[&str]) -> UpdateLoginPolicyRequest {
        UpdateLoginPolicyRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            ..Default::default()
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 reason 时，载荷里
    /// 即便带了 value/method 也不许动；type 的列名是 type_column，照样
    /// 按 wire 名 "type" 过门。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = LoginPolicyService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "1.2.3.4").await;

        svc.update(
            ctx(1, 9),
            update_req(
                row.id as u64,
                LoginPolicy {
                    reason: Some("新原因".into()),
                    value: Some("5.6.7.8".into()),
                    method: Some(2),
                    r#type: Some(2),
                    ..Default::default()
                },
                &["reason"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.reason.as_deref(),
            Some("新原因"),
            "masked reason lands"
        );
        assert_eq!(
            fresh.value.as_deref(),
            Some("1.2.3.4"),
            "unmasked value is not writable through the gate"
        );
        assert_eq!(
            fresh.method.as_deref(),
            Some("IP"),
            "unmasked method untouched"
        );
        assert_eq!(
            fresh.type_column.as_deref(),
            Some("BLACKLIST"),
            "unmasked type (type_column) untouched"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = LoginPolicyService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, 1, "10.0.0.1").await;

        // 无掩码：None 不动列。
        svc.update(
            ctx(1, 9),
            UpdateLoginPolicyRequest {
                id: row.id as u64,
                data: Some(LoginPolicy {
                    value: Some("10.0.0.2".into()),
                    ..Default::default()
                }),
                update_mask: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.reason.as_deref(),
            Some("原始原因"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            ctx(1, 9),
            update_req(row.id as u64, LoginPolicy::default(), &["reason"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.reason.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
