//! TenantService — //! internal/service/service: tenant CRUD, the with-admin
//! provisioning (tenant + template role + admin user + credential in one
//! transaction, then AssignTenantAdmin semantics), TenantExists (code OR
//! name), usage counts, and CleanupData (wipe tenant-scoped rows, park
//! the tenant OFF, revoke every user's sessions).

use std::sync::Arc;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};

use crate::mapping;
use crate::state::{
    db_err, internal_error, not_found, operator_of, status_error, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::{
    CleanupTenantDataRequest, CreateTenantRequest, CreateTenantWithAdminUserRequest,
    DeleteTenantRequest, GetTenantRequest, GetTenantUsageRequest, ListTenantResponse, Tenant,
    TenantExistsRequest, TenantExistsResponse, TenantUsage, UpdateTenantRequest,
};
use proto::proto::pagination::PagingRequest;

/// Unknown rows read as ON.
fn status_to_proto(s: &str) -> i32 {
    mapping::tenant_status_of(s).unwrap_or(0)
}

fn status_to_str(v: i32) -> String {
    mapping::tenant_status_str(v).unwrap_or("ON").into()
}

/// Unknown rows read as PAID.
fn type_to_proto(s: &str) -> i32 {
    mapping::tenant_type_of(s).unwrap_or(1)
}

fn type_to_str(v: i32) -> String {
    mapping::tenant_type_str(v).unwrap_or("PAID").into()
}

fn tenant_proto(r: crate::data::sys_tenants::Model) -> Tenant {
    Tenant {
        id: Some(r.id as u64),
        name: Some(r.name),
        code: Some(r.code),
        domain: r.domain,
        logo_url: r.logo_url,
        industry: r.industry,
        r#type: r.type_column.as_deref().map(type_to_proto),
        remark: r.remark,
        admin_user_id: r.admin_user_id.map(|v| v as u64),
        admin_user_name: None,
        subscription_at: r.subscription_at.and_then(crate::state::naive_to_ts),
        unsubscribe_at: r.unsubscribe_at.and_then(crate::state::naive_to_ts),
        expired_at: r.expired_at.and_then(crate::state::naive_to_ts),
        subscription_plan: r.subscription_plan,
        plan_id: r.plan_id.map(|v| v as u64),
        member_count: None,
        status: r.status.as_deref().map(status_to_proto),
        audit_status: r.audit_status.as_deref().map(|s| match s {
            "APPROVED" => 1,
            "REJECTED" => 2,
            _ => 0,
        }),
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct TenantService {
    pub state: Arc<AppState>,
}

impl TenantService {
    /// 平台管理员闸（requirePlatformAdmin 同语义）：租户名录/详情/查重是
    /// 平台专属读口，租户侧自查走 MyTenantUsageService。
    async fn require_platform_admin(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        action: &str,
    ) -> Result<(), StatusError> {
        let payload = crate::state::operator_of(ctx)?;
        if payload.is_platform_admin != Some(true) {
            return Err(status_error(
                "FORBIDDEN",
                format!("only platform admin can {action}"),
            ));
        }
        Ok(())
    }

    async fn find(
        &self,
        query: proto::proto::identity::service::v1::get_tenant_request::QueryBy,
    ) -> Result<Option<crate::data::sys_tenants::Model>, StatusError> {
        use proto::proto::identity::service::v1::get_tenant_request::QueryBy;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        match query {
            QueryBy::Id(id) => repo.find_by_id(id as i64).await,
            QueryBy::Code(code) => repo.find_by_code(&code).await,
            QueryBy::Name(name) => repo.find_by_name(&name).await,
        }
    }

    /// The with-admin provisioning inside one transaction.
    async fn provision_with_admin(
        &self,
        txn: &DatabaseTransaction,
        tenant_data: Tenant,
        admin_user: proto::proto::identity::service::v1::User,
        password: &str,
        operator_id: i64,
    ) -> Result<(), StatusError> {
        // 1. tenant row.
        let tenant = crate::data::sys_tenants::ActiveModel {
            name: Set(tenant_data.name.clone().unwrap_or_default()),
            code: Set(tenant_data.code.clone().unwrap_or_default()),
            domain: Set(tenant_data.domain.clone()),
            logo_url: Set(tenant_data.logo_url.clone()),
            industry: Set(tenant_data.industry.clone()),
            type_column: Set(Some(
                tenant_data
                    .r#type
                    .map(type_to_str)
                    .unwrap_or_else(|| "PAID".into()),
            )),
            status: Set(Some("ON".into())),
            audit_status: Set(Some("APPROVED".into())),
            remark: Set(tenant_data.remark.clone()),
            expired_at: Set(tenant_data
                .expired_at
                .as_ref()
                .and_then(crate::state::ts_to_naive)),
            plan_id: Set(tenant_data.plan_id.map(|v| v as i64)),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;

        // 2. the tenant-manager role copied from the template.
        let template = crate::data::sys_roles::Entity::find()
            .filter(crate::data::sys_roles::Column::Code.eq("template:tenant:manager"))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| internal_error("tenant-manager role template missing"))?;
        let role = crate::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            name: Set(template.name.clone()),
            code: Set("tenant:manager".into()),
            is_protected: Set(Some(true)),
            type_column: Set(Some("TENANT".into())),
            data_scope: Set(template.data_scope.clone()),
            status: Set(Some("ON".into())),
            sort_order: Set(template.sort_order),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        // Template permissions ride the copy.
        let template_perms = crate::data::sys_role_permissions::Entity::find()
            .filter(crate::data::sys_role_permissions::Column::RoleId.eq(template.id))
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        for perm in template_perms {
            crate::data::sys_role_permissions::ActiveModel {
                tenant_id: Set(Some(tenant.id)),
                role_id: Set(Some(role.id)),
                permission_id: Set(perm.permission_id),
                effect: Set(Some("ALLOW".into())),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(txn)
            .await
            .map_err(db_err)?;
        }

        // 3. the admin user (+ credential with the plaintext password).
        let username = admin_user.username.clone().unwrap_or_default();
        let user = crate::data::sys_users::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            username: Set(username.clone()),
            realname: Set(admin_user.realname.clone()),
            nickname: Set(admin_user.nickname.clone()),
            email: Set(admin_user.email.clone()),
            mobile: Set(admin_user.mobile.clone()),
            status: Set(Some("NORMAL".into())),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        crate::data::sys_user_credentials::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            user_id: Set(Some(user.id)),
            identity_type: Set(Some("USERNAME".into())),
            identifier: Set(username),
            credential_type: Set(Some("PASSWORD_HASH".into())),
            credential: Set(crate::crypto::hash_password(password).map_err(internal_error)?),
            is_primary: Set(Some(true)),
            status: Set(Some("ENABLED".into())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;

        // 4. AssignTenantAdmin: primary binding + back-pointer.
        crate::data::sys_user_roles::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            user_id: Set(Some(user.id)),
            role_id: Set(Some(role.id)),
            is_primary: Set(Some(true)),
            status: Set(Some("ACTIVE".into())),
            assigned_by: Set(Some(operator_id)),
            assigned_at: Set(Some(crate::data::now())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        let mut tenant_update: crate::data::sys_tenants::ActiveModel = tenant.into();
        tenant_update.admin_user_id = Set(Some(user.id));
        tenant_update.update(txn).await.map_err(db_err)?;
        Ok(())
    }

    /// CleanupData: delete every tenant-scoped row, keep the tenant row
    /// parked OFF, then revoke every user's sessions.
    async fn cleanup_rows(&self, tenant_id: i64) -> Result<(), StatusError> {
        use crate::data::*;
        macro_rules! wipe {
            ($($entity:ident)::+) => {
                $($entity)::+::Entity::delete_many()
                    .filter($($entity)::+::Column::TenantId.eq(tenant_id))
                    .exec(&self.state.db)
                    .await
                    .map_err(db_err)?;
            };
        }
        wipe!(sys_user_roles);
        wipe!(sys_user_credentials);
        wipe!(sys_user_mfa_factors);
        wipe!(sys_roles);
        wipe!(sys_role_permissions);
        wipe!(sys_role_org_units);
        wipe!(sys_role_field_permissions);
        wipe!(sys_role_metadata);
        wipe!(sys_users);
        wipe!(sys_org_units);
        wipe!(sys_positions);
        wipe!(sys_dict_types);
        wipe!(sys_dict_entries);
        wipe!(sys_dict_entry_i18n);
        wipe!(sys_tasks);
        wipe!(sys_login_policies);
        wipe!(sys_access_keys);
        wipe!(internal_message_recipients);
        wipe!(internal_messages);
        wipe!(internal_message_categories);
        wipe!(files);
        // The audit tables live under the audit submodule.
        use crate::data::{
            sys_api_audit_logs, sys_data_access_audit_logs, sys_login_audit_logs,
            sys_operation_audit_logs, sys_permission_audit_logs, sys_policy_evaluation_logs,
        };
        wipe!(sys_api_audit_logs);
        wipe!(sys_operation_audit_logs);
        wipe!(sys_login_audit_logs);
        wipe!(sys_data_access_audit_logs);
        wipe!(sys_permission_audit_logs);
        wipe!(sys_policy_evaluation_logs);
        Ok(())
    }
}

#[async_trait::async_trait]
impl proto::gen::services::TenantServiceHandlers for TenantService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListTenantResponse, StatusError> {
        // 租户名录含全平台客户名称/状态/到期/套餐——平台专属读口
        // （P2 收闸；租户侧自查走 MyTenantUsageService，不经本服务）。
        self.require_platform_admin(&ctx, "tenant list").await?;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListTenantResponse {
            items: rows.into_iter().map(tenant_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetTenantRequest,
    ) -> Result<Tenant, StatusError> {
        // 平台专属读口（P2 收闸，与 list 同口径）。
        self.require_platform_admin(&ctx, "tenant detail").await?;
        let Some(query) = req.query_by else {
            return Err(status_error("BAD_REQUEST", "query_by required"));
        };
        let row = self.find(query).await?.ok_or_else(|| not_found("tenant"))?;
        Ok(tenant_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateTenantRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        // The exists gate: code OR name.
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        if let Some(code) = &data.code {
            if repo.find_by_code(code).await?.is_some() {
                return Err(status_error("BAD_REQUEST", "tenant already exists"));
            }
        }
        if let Some(name) = &data.name {
            if repo.find_by_name(name).await?.is_some() {
                return Err(status_error("BAD_REQUEST", "tenant already exists"));
            }
        }
        crate::data::sys_tenants::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
            domain: Set(data.domain.clone()),
            logo_url: Set(data.logo_url.clone()),
            industry: Set(data.industry.clone()),
            type_column: Set(Some(
                data.r#type
                    .map(type_to_str)
                    .unwrap_or_else(|| "PAID".into()),
            )),
            status: Set(Some("ON".into())),
            audit_status: Set(Some("APPROVED".into())),
            remark: Set(data.remark.clone()),
            expired_at: Set(data.expired_at.as_ref().and_then(crate::state::ts_to_naive)),
            plan_id: Set(data.plan_id.map(|v| v as i64)),
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
        req: UpdateTenantRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id(req.id as i64)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        let mut a: crate::data::sys_tenants::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.domain {
                a.domain = Set(Some(v.clone()));
            }
            if let Some(v) = &data.logo_url {
                a.logo_url = Set(Some(v.clone()));
            }
            if let Some(v) = &data.industry {
                a.industry = Set(Some(v.clone()));
            }
            if let Some(v) = &data.remark {
                a.remark = Set(Some(v.clone()));
            }
            if let Some(v) = data.r#type {
                a.type_column = Set(Some(type_to_str(v)));
            }
            if let Some(v) = data.status {
                a.status = Set(Some(status_to_str(v)));
            }
            if let Some(v) = data.plan_id {
                a.plan_id = Set(Some(v as i64));
            }
            if let Some(v) = &data.expired_at {
                a.expired_at = Set(crate::state::ts_to_naive(v));
            }
        }
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteTenantRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::identity::service::v1::delete_tenant_request::QueryBy
        );
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id(id as i64)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        // Cleanup semantics ride along: the tenants themselves are parked,
        // not deleted (module Delete → cleanup + OFF).
        self.cleanup_rows(row.id).await?;
        let mut a: crate::data::sys_tenants::ActiveModel = row.into();
        a.status = Set(Some("OFF".into()));
        a.deleted_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn create_tenant_with_admin_user(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateTenantWithAdminUserRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let tenant_data = req
            .tenant
            .clone()
            .ok_or_else(|| status_error("BAD_REQUEST", "tenant required"))?;
        let admin = req
            .user
            .clone()
            .ok_or_else(|| status_error("BAD_REQUEST", "user required"))?;
        if req.password.is_empty() {
            return Err(status_error("BAD_REQUEST", "password required"));
        }
        // The exists gate (code OR name).
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let exists = repo
            .exists_values_any(
                &tenant_data.code.clone().unwrap_or_default(),
                &tenant_data.name.clone().unwrap_or_default(),
            )
            .await?;
        if exists {
            return Err(status_error("BAD_REQUEST", "tenant already exists"));
        }
        let txn = self.state.db.begin().await.map_err(db_err)?;
        if let Err(e) = self
            .provision_with_admin(&txn, tenant_data, admin, &req.password, payload.user_id)
            .await
        {
            txn.rollback().await.map_err(db_err)?;
            return Err(e);
        }
        txn.commit().await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn tenant_exists(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: TenantExistsRequest,
    ) -> Result<TenantExistsResponse, StatusError> {
        // 唯一消费方是平台管理员的租户抽屉 code/name 查重，随读口一并收闸。
        self.require_platform_admin(&ctx, "tenant exists").await?;
        // OR semantics.
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let exist = repo.exists_any(&req.code, &req.name).await?;
        Ok(TenantExistsResponse { exist })
    }

    async fn get_usage(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetTenantUsageRequest,
    ) -> Result<TenantUsage, StatusError> {
        usage_for(&self.state, req.id as i64).await
    }

    async fn cleanup_data(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CleanupTenantDataRequest,
    ) -> Result<Empty, StatusError> {
        let _ = operator_of(&ctx)?;
        self.cleanup_rows(req.id as i64).await?;
        // Park the tenant, keep the row.
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        if let Some(row) = repo.find_by_id(req.id as i64).await? {
            let mut a: crate::data::sys_tenants::ActiveModel = row.into();
            a.status = Set(Some("OFF".into()));
            a.updated_at = Set(Some(crate::data::now()));
            a.update(&self.state.db).await.map_err(db_err)?;
        }
        Ok(Empty {})
    }
}

/// 用量与配额计算：平台管理员按租户查（TenantService.GetUsage）与租户
/// 自助查（MyTenantUsageService.GetMyTenantUsage）共享同一内核。
/// 存储用量/API 调用计数与配额行随各自的计费面里程碑接入，现读零值。
pub(crate) async fn usage_for(
    state: &Arc<AppState>,
    tenant_id: i64,
) -> Result<TenantUsage, StatusError> {
    let repo = crate::data::repos::TenantRepo::new(&state.db);
    let user_count = repo.user_count(tenant_id).await;
    let tenant = repo.find_by_id(tenant_id).await?;
    let plan_name = match tenant.as_ref().and_then(|t| t.plan_id) {
        Some(pid) => crate::data::sys_plans::Entity::find_by_id(pid)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|p| p.name),
        None => None,
    };
    Ok(TenantUsage {
        tenant_id: tenant_id as u64,
        user_count,
        storage_used_bytes: 0,
        api_call_count: 0,
        plan_id: tenant.and_then(|t| t.plan_id).map(|v| v as u64),
        plan_name,
        quotas: Vec::new(),
    })
}
