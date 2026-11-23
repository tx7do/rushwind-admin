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

use crate::admin::mapping;
use crate::admin::state::{
    db_err, internal_error, not_found, operator_of, status_error, AppState, StatusError,
};
use crate::proto::identity::service::v1::{
    CleanupTenantDataRequest, CreateTenantRequest, CreateTenantWithAdminUserRequest,
    DeleteTenantRequest, GetTenantRequest, GetTenantUsageRequest, ListTenantResponse, Tenant,
    TenantExistsRequest, TenantExistsResponse, TenantUsage, UpdateTenantRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

/// Unknown rows read as ON.
fn status_to_proto(s: &str) -> i32 {
    mapping::tenant_status_of(s).unwrap_or(1)
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

/// Unknown rows read as PENDING (the column default).
fn audit_status_to_proto(s: &str) -> i32 {
    mapping::tenant_audit_status_of(s).unwrap_or(1)
}

fn audit_status_to_str(v: i32) -> String {
    mapping::tenant_audit_status_str(v)
        .unwrap_or("PENDING")
        .into()
}

fn tenant_proto(r: crate::admin::data::sys_tenants::Model) -> Tenant {
    Tenant {
        id: Some(r.id),
        name: Some(r.name),
        code: Some(r.code),
        domain: r.domain,
        logo_url: r.logo_url,
        industry: r.industry,
        r#type: r.type_column.as_deref().map(type_to_proto),
        remark: r.remark,
        admin_user_id: r.admin_user_id,
        admin_user_name: None,
        subscription_at: r.subscription_at.and_then(crate::admin::state::naive_to_ts),
        unsubscribe_at: r.unsubscribe_at.and_then(crate::admin::state::naive_to_ts),
        expired_at: r.expired_at.and_then(crate::admin::state::naive_to_ts),
        subscription_plan: r.subscription_plan,
        plan_id: r.plan_id,
        member_count: None,
        status: r.status.as_deref().map(status_to_proto),
        audit_status: r.audit_status.as_deref().map(audit_status_to_proto),
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
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
        let payload = crate::admin::state::operator_of(ctx)?;
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
        query: crate::proto::identity::service::v1::get_tenant_request::QueryBy,
    ) -> Result<Option<crate::admin::data::sys_tenants::Model>, StatusError> {
        use crate::proto::identity::service::v1::get_tenant_request::QueryBy;
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        match query {
            QueryBy::Id(id) => repo.find_by_id(id).await,
            QueryBy::Code(code) => repo.find_by_code(&code).await,
            QueryBy::Name(name) => repo.find_by_name(&name).await,
        }
    }

    /// The with-admin provisioning inside one transaction.
    async fn provision_with_admin(
        &self,
        txn: &DatabaseTransaction,
        tenant_data: Tenant,
        admin_user: crate::proto::identity::service::v1::User,
        password: &str,
        operator_id: i64,
    ) -> Result<(), StatusError> {
        // 1. tenant row.
        let tenant = crate::admin::data::sys_tenants::ActiveModel {
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
                .and_then(crate::admin::state::ts_to_naive)),
            plan_id: Set(tenant_data.plan_id),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;

        // 2. the tenant-manager role copied from the template.
        let template = crate::admin::data::sys_roles::Entity::find()
            .filter(crate::admin::data::sys_roles::Column::Code.eq("template:tenant:manager"))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| internal_error("tenant-manager role template missing"))?;
        let role = crate::admin::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            name: Set(template.name.clone()),
            code: Set("tenant:manager".into()),
            is_protected: Set(Some(true)),
            type_column: Set(Some("TENANT".into())),
            data_scope: Set(template.data_scope.clone()),
            home_path: Set(template.home_path.clone()),
            status: Set(Some("ON".into())),
            sort_order: Set(template.sort_order),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        // Template permissions ride the copy.
        let template_perms = crate::admin::data::sys_role_permissions::Entity::find()
            .filter(crate::admin::data::sys_role_permissions::Column::RoleId.eq(template.id))
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        for perm in template_perms {
            crate::admin::data::sys_role_permissions::ActiveModel {
                tenant_id: Set(Some(tenant.id)),
                role_id: Set(Some(role.id)),
                permission_id: Set(perm.permission_id),
                effect: Set(Some("ALLOW".into())),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::admin::data::now())),
                updated_at: Set(Some(crate::admin::data::now())),
                ..Default::default()
            }
            .insert(txn)
            .await
            .map_err(db_err)?;
        }

        // 3. the admin user (+ credential with the plaintext password).
        let username = admin_user.username.clone().unwrap_or_default();
        let user = crate::admin::data::sys_users::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            username: Set(username.clone()),
            realname: Set(admin_user.realname.clone()),
            nickname: Set(admin_user.nickname.clone()),
            email: Set(admin_user.email.clone()),
            mobile: Set(admin_user.mobile.clone()),
            status: Set(Some("NORMAL".into())),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        crate::admin::data::sys_user_credentials::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            user_id: Set(Some(user.id)),
            identity_type: Set(Some("USERNAME".into())),
            identifier: Set(username),
            credential_type: Set(Some("PASSWORD_HASH".into())),
            credential: Set(crate::admin::crypto::hash_password(password).map_err(internal_error)?),
            is_primary: Set(Some(true)),
            status: Set(Some("ENABLED".into())),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;

        // 4. AssignTenantAdmin: primary binding + back-pointer.
        crate::admin::data::sys_user_roles::ActiveModel {
            tenant_id: Set(Some(tenant.id)),
            user_id: Set(Some(user.id)),
            role_id: Set(Some(role.id)),
            is_primary: Set(Some(true)),
            status: Set(Some("ACTIVE".into())),
            assigned_by: Set(Some(operator_id)),
            assigned_at: Set(Some(crate::admin::data::now())),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(txn)
        .await
        .map_err(db_err)?;
        let mut tenant_update: crate::admin::data::sys_tenants::ActiveModel = tenant.into();
        tenant_update.admin_user_id = Set(Some(user.id));
        tenant_update.update(txn).await.map_err(db_err)?;
        Ok(())
    }

    /// CleanupData: delete every tenant-scoped row, keep the tenant row
    /// parked OFF, then revoke every user's sessions.
    async fn cleanup_rows(&self, tenant_id: i64) -> Result<(), StatusError> {
        use crate::admin::data::*;
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
        // The per-tenant rows of the ai/bi/print faces were missing from the
        // wipe list — their tenant_id dangled after a delete (issue #3 purge
        // must not leave cross-face residue either).
        wipe!(sys_ai_providers);
        wipe!(sys_ai_agent_apps);
        wipe!(sys_ai_agent_runs);
        wipe!(sys_ai_conversations);
        wipe!(sys_ai_messages);
        wipe!(sys_ai_knowledge_bases);
        wipe!(sys_ai_docs);
        wipe!(sys_ai_chunks);
        wipe!(sys_ai_usage_logs);
        wipe!(sys_ai_agent_revisions);
        wipe!(bi_dashboards);
        wipe!(bi_dashboard_revisions);
        wipe!(print_templates);
        wipe!(internal_message_recipients);
        wipe!(internal_messages);
        wipe!(internal_message_categories);
        wipe!(files);
        // The audit tables live under the audit submodule.
        use crate::admin::data::{
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
impl crate::proto::gen::services::TenantServiceHandlers for TenantService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListTenantResponse, StatusError> {
        // 租户名录含全平台客户名称/状态/到期/套餐——平台专属读口
        // （P2 收闸；租户侧自查走 MyTenantUsageService，不经本服务）。
        self.require_platform_admin(&ctx, "tenant list").await?;
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListTenantResponse {
            items: rows.into_iter().map(tenant_proto).collect(),
            total: total as i64,
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
        // 平台专属写口（P2 收闸补齐）：租户是平台资产，读写两侧同口径。
        self.require_platform_admin(&ctx, "tenant create").await?;
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        // The exists gate: code OR name.
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
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
        crate::admin::data::sys_tenants::ActiveModel {
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
            // The drawer's selection wins; API callers omitting the field keep
            // the historical admin-created default (APPROVED).
            audit_status: Set(Some(
                data.audit_status
                    .filter(|v| *v != 0)
                    .map(audit_status_to_str)
                    .unwrap_or_else(|| "APPROVED".into()),
            )),
            remark: Set(data.remark.clone()),
            expired_at: Set(data
                .expired_at
                .as_ref()
                .and_then(crate::admin::state::ts_to_naive)),
            plan_id: Set(data.plan_id),
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
        req: UpdateTenantRequest,
    ) -> Result<Empty, StatusError> {
        // 平台专属写口（P2 收闸补齐）：原实现只验身份，任何登录用户可改
        // 任意租户的 status/plan/expired_at。
        self.require_platform_admin(&ctx, "tenant update").await?;
        let _ = operator_of(&ctx)?;
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id(req.id)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        let mut a: crate::admin::data::sys_tenants::ActiveModel = row.into();
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
            crate::set_masked!(opt mask, a, domain, data.domain, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, logo_url, data.logo_url, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, industry, data.industry, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt_named mask, a, type_column, "type", data.r#type, |v: &i32| Some(
                type_to_str(*v)
            ));
            crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(
                status_to_str(*v)
            ));
            // UNSPECIFIED (0) carries no opinion on a partial update——过滤
            // 后的载荷 None（含 0）在掩码下也不写列（审核态无清值语义）。
            crate::set_masked!(
                req_named mask,
                a,
                audit_status,
                "audit_status",
                data.audit_status.filter(|v| *v != 0),
                |v: &i32| Some(audit_status_to_str(*v))
            );
            crate::set_masked!(opt mask, a, plan_id, data.plan_id, |v: &i64| Some(
                *v
            ));
            crate::set_masked!(opt mask, a, expired_at, data.expired_at, |v: &pbjson_types::Timestamp| {
                crate::state::ts_to_naive(v)
            });
        }
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteTenantRequest,
    ) -> Result<Empty, StatusError> {
        // 平台专属写口（P2 收闸补齐）：delete 触发 cleanup_rows 级联清数，
        // 是全系统破坏面最大的单口。
        self.require_platform_admin(&ctx, "tenant delete").await?;
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::identity::service::v1::delete_tenant_request::QueryBy
        );
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        // Cleanup semantics ride along: the tenants themselves are parked,
        // not deleted (module Delete → cleanup + OFF). deleted_by stamps the
        // operator so the recycle-bin face can show who parked it (issue #3).
        let payload = operator_of(&ctx)?;
        self.cleanup_rows(row.id).await?;
        let mut a: crate::admin::data::sys_tenants::ActiveModel = row.into();
        a.status = Set(Some("OFF".into()));
        a.deleted_by = Set(Some(payload.user_id));
        a.deleted_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    /// 回收站列表：停车行（deleted_at 非空），删除时间降序（issue #3）。
    /// 与 list 同口径的平台专属读口。
    async fn list_deleted_tenants(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListTenantResponse, StatusError> {
        self.require_platform_admin(&ctx, "tenant trash list")
            .await?;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let (rows, total) = repo.paged_deleted(&req).await?;
        Ok(ListTenantResponse {
            items: rows.into_iter().map(tenant_proto).collect(),
            total: total as i64,
        })
    }

    /// 回收站恢复：清删除标记并置回在营态（issue #3）。停车租户恢复后是
    /// 空壳（delete 时关联行已级联清理，admin_user_id 指向已删用户仅是
    /// 历史痕迹）——平台管理员恢复后需另行补管理员（create_tenant_with_admin
    /// 面向新租户，不适用）；恢复前查重 alive 域的 code/name，防止停车期间
    /// 被新建租户占用。
    async fn restore_tenant(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::identity::service::v1::RestoreTenantRequest,
    ) -> Result<Empty, StatusError> {
        self.require_platform_admin(&ctx, "tenant restore").await?;
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id_unfiltered(req.id)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        if row.deleted_at.is_none() {
            return Err(status_error("BAD_REQUEST", "tenant is not parked"));
        }
        // alive 域查重：code/name 被在营租户占用即拒绝（与 create 同文案）。
        if !row.code.is_empty() && repo.find_by_code(&row.code).await?.is_some() {
            return Err(status_error("BAD_REQUEST", "tenant already exists"));
        }
        if !row.name.is_empty() && repo.find_by_name(&row.name).await?.is_some() {
            return Err(status_error("BAD_REQUEST", "tenant already exists"));
        }
        let mut a: crate::data::sys_tenants::ActiveModel = row.into();
        a.status = Set(Some("ON".into()));
        a.deleted_at = Set(None);
        a.deleted_by = Set(None);
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    /// 回收站彻底删除：级联清残余关联行后硬删租户行，不可逆（issue #3）。
    /// 只对停车行生效——在营租户请走 delete（停车）而非直接抹除。
    async fn purge_tenant(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: crate::proto::identity::service::v1::PurgeTenantRequest,
    ) -> Result<Empty, StatusError> {
        self.require_platform_admin(&ctx, "tenant purge").await?;
        let repo = crate::data::repos::TenantRepo::new(&self.state.db);
        let row = repo
            .find_by_id_unfiltered(req.id)
            .await?
            .ok_or_else(|| not_found("tenant"))?;
        if row.deleted_at.is_none() {
            return Err(status_error(
                "BAD_REQUEST",
                "tenant is not parked; delete it first",
            ));
        }
        self.cleanup_rows(row.id).await?;
        crate::data::sys_tenants::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn create_tenant_with_admin_user(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateTenantWithAdminUserRequest,
    ) -> Result<Empty, StatusError> {
        // 平台专属写口（P2 收闸补齐）：与 create 同口径。
        self.require_platform_admin(&ctx, "tenant create").await?;
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
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
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
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        let exist = repo.exists_any(&req.code, &req.name).await?;
        Ok(TenantExistsResponse { exist })
    }

    async fn get_usage(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetTenantUsageRequest,
    ) -> Result<TenantUsage, StatusError> {
        // 归属闸：平台管理员看任意租户用量，租户成员只看自己（防跨租户
        // 用量探测）。
        let payload = operator_of(&ctx)?;
        let tid = req.id;
        if payload.is_platform_admin != Some(true) && payload.tenant_id != tid {
            return Err(status_error(
                "FORBIDDEN",
                "cannot read another tenant's usage",
            ));
        }
        usage_for(&self.state, tid).await
    }

    async fn cleanup_data(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CleanupTenantDataRequest,
    ) -> Result<Empty, StatusError> {
        // 平台专属写口（P2 收闸补齐）：cleanup_rows 级联清 30+ 张表。
        self.require_platform_admin(&ctx, "tenant cleanup").await?;
        let _ = operator_of(&ctx)?;
        self.cleanup_rows(req.id).await?;
        // Park the tenant, keep the row.
        let repo = crate::admin::data::repos::TenantRepo::new(&self.state.db);
        if let Some(row) = repo.find_by_id(req.id).await? {
            let mut a: crate::admin::data::sys_tenants::ActiveModel = row.into();
            a.status = Set(Some("OFF".into()));
            a.updated_at = Set(Some(crate::admin::data::now()));
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
    let repo = crate::admin::data::repos::TenantRepo::new(&state.db);
    let user_count = repo.user_count(tenant_id).await;
    let tenant = repo.find_by_id(tenant_id).await?;
    let plan_name = match tenant.as_ref().and_then(|t| t.plan_id) {
        Some(pid) => crate::admin::data::sys_plans::Entity::find_by_id(pid)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|p| p.name),
        None => None,
    };
    Ok(TenantUsage {
        tenant_id,
        user_count: user_count as i64,
        storage_used_bytes: 0,
        api_call_count: 0,
        plan_id: tenant.and_then(|t| t.plan_id),
        plan_name,
        quotas: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::gen::services::TenantServiceHandlers as _;
    use crate::proto::identity::service::v1::delete_tenant_request::QueryBy as DeleteQueryBy;
    use crate::testsupport::{ctx, test_state};

    /// 平台管理员 ctx（tid=0 + ipa=true）：租户名录/详情是平台专属读口。
    fn admin_ctx() -> rushwind_http_binding::ctx::RequestContext {
        let mut c = ctx(0, 1);
        if let Some(claims) = c.claims.as_mut() {
            claims.insert("ipa".into(), serde_json::json!(true));
        }
        c
    }

    fn tenant(name: &str, audit: Option<i32>) -> Tenant {
        Tenant {
            name: Some(name.into()),
            code: Some(name.into()),
            audit_status: audit,
            ..Default::default()
        }
    }

    /// 审核状态全链往返（issue #6a）：create 尊重入参、读面按 proto 枚举
    /// 原样返回（1=PENDING/2=APPROVED/3=REJECTED）、update 可改——保存的
    /// 是「已通过」就不再显示「待审核」。删除是停车不是消失：列表不再
    /// 出现（issue #6b）。
    #[tokio::test]
    async fn audit_status_round_trip_and_parked_invisibility() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = admin_ctx();

        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("acme", Some(2))),
            },
        )
        .await
        .unwrap();
        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("globex", Some(1))),
            },
        )
        .await
        .unwrap();

        let listed = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        let acme = listed
            .items
            .iter()
            .find(|t| t.code.as_deref() == Some("acme"))
            .expect("acme listed");
        assert_eq!(
            acme.audit_status,
            Some(2),
            "created APPROVED reads APPROVED"
        );
        let globex = listed
            .items
            .iter()
            .find(|t| t.code.as_deref() == Some("globex"))
            .expect("globex listed");
        assert_eq!(
            globex.audit_status,
            Some(1),
            "created PENDING reads PENDING"
        );
        let acme_id = acme.id.unwrap() as i64;

        // The drawer's change sticks: APPROVED -> REJECTED.
        svc.update(
            me.clone(),
            UpdateTenantRequest {
                id: acme_id,
                data: Some(Tenant {
                    audit_status: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let reread = svc
            .get(
                me.clone(),
                GetTenantRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_tenant_request::QueryBy::Id(
                            acme_id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(reread.audit_status, Some(3), "update audit_status persists");

        // Delete parks the row: list and the exists gate both forget it.
        svc.delete(
            me.clone(),
            DeleteTenantRequest {
                query_by: Some(DeleteQueryBy::Id(acme_id)),
            },
        )
        .await
        .unwrap();
        let after = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        assert_eq!(after.total, 1, "parked tenant leaves the list");
        assert!(after.items.iter().all(|t| t.id != Some(acme_id)));
        // Recreating the parked tenant's code passes the exists gate.
        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("acme", Some(2))),
            },
        )
        .await
        .expect("parked code/name is reusable");
    }

    /// 回收站三操作全链（issue #3）：删除停车 → trash 列表可见 → code 被
    /// 在营租户占用时 restore 拒绝 → purge 硬删（unfiltered 也不再可见，
    /// 关联行随级联清空）→ 在营行直接 purge 被拒。
    #[tokio::test]
    async fn recycle_bin_chain() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = admin_ctx();

        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("acme", Some(2))),
            },
        )
        .await
        .unwrap();
        let listed = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        let acme_id = listed.items[0].id.unwrap() as i64;

        // 停车。
        svc.delete(
            me.clone(),
            DeleteTenantRequest {
                query_by: Some(DeleteQueryBy::Id(acme_id)),
            },
        )
        .await
        .unwrap();

        // trash 列表可见。
        let trash = svc
            .list_deleted_tenants(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        assert_eq!(trash.total, 1, "parked tenant shows in trash");
        assert_eq!(trash.items[0].id, Some(acme_id));
        assert!(trash.items[0].deleted_by.is_some(), "deleted_by stamped");

        // 在营对照：占用者与 acme 同 code/name（停车腾出唯一性后才建得成
        // ——这正是 #6b 的 exists 闸语义，也构成 restore 的冲突场景）。
        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("acme", Some(1))),
            },
        )
        .await
        .expect("parked code/name is reusable");
        let alive = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        let occupant_id = alive.items[0].id.unwrap() as i64;

        // 在营行直接 purge 被拒。
        let err = svc
            .purge_tenant(
                me.clone(),
                crate::proto::identity::service::v1::PurgeTenantRequest { id: occupant_id },
            )
            .await
            .unwrap_err();
        assert_eq!(
            err.reason, "BAD_REQUEST",
            "purging a live tenant is rejected"
        );

        // code 被在营租户占用时 restore 拒绝。
        let err = svc
            .restore_tenant(
                me.clone(),
                crate::proto::identity::service::v1::RestoreTenantRequest { id: acme_id },
            )
            .await
            .unwrap_err();
        assert_eq!(err.reason, "BAD_REQUEST", "occupied code blocks restore");

        // purge 掉占用者后再 restore 成功。
        svc.delete(
            me.clone(),
            DeleteTenantRequest {
                query_by: Some(DeleteQueryBy::Id(occupant_id)),
            },
        )
        .await
        .unwrap();
        svc.purge_tenant(
            me.clone(),
            crate::proto::identity::service::v1::PurgeTenantRequest { id: occupant_id },
        )
        .await
        .unwrap();
        let raw = crate::data::repos::TenantRepo::new(&state.db)
            .find_by_id_unfiltered(occupant_id)
            .await
            .unwrap();
        assert!(raw.is_none(), "purged row is gone even unfiltered");
        svc.restore_tenant(
            me.clone(),
            crate::proto::identity::service::v1::RestoreTenantRequest { id: acme_id },
        )
        .await
        .unwrap();
        let restored = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        assert_eq!(
            restored.total, 1,
            "acme back on the alive face, occupant purged"
        );
        assert_eq!(
            restored.items[0].deleted_by, None,
            "restore clears the deletion stamp"
        );
        assert_eq!(
            restored.items[0].status,
            Some(1),
            "restored tenant reads ON"
        );
        // 空壳语义：停车时关联行已被级联清理。
        let repo = crate::data::repos::TenantRepo::new(&state.db);
        assert_eq!(repo.user_count(acme_id).await, 0, "restore is a shell");
    }

    /// 回收站三操作全部平台闸：非平台管理员 403（对齐 list/get 的 P2 闸）。
    #[tokio::test]
    async fn recycle_bin_is_platform_gated() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = ctx(7, 1); // 非 ipa
        for err in [
            svc.list_deleted_tenants(me.clone(), PagingRequest::default())
                .await
                .unwrap_err()
                .reason,
            svc.restore_tenant(
                me.clone(),
                crate::proto::identity::service::v1::RestoreTenantRequest { id: 1 },
            )
            .await
            .unwrap_err()
            .reason,
            svc.purge_tenant(
                me.clone(),
                crate::proto::identity::service::v1::PurgeTenantRequest { id: 1 },
            )
            .await
            .unwrap_err()
            .reason,
        ] {
            assert_eq!(err, "FORBIDDEN");
        }
    }

    /// update_mask 白名单门：掩码只选 domain 时，载荷里多带的 industry/
    /// remark 不许落列。
    #[tokio::test]
    async fn mask_gates_unlisted_tenant_fields() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = admin_ctx();

        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("acme", Some(2))),
            },
        )
        .await
        .unwrap();
        let listed = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        let acme_id = listed.items[0].id.unwrap();

        svc.update(
            me.clone(),
            UpdateTenantRequest {
                id: acme_id,
                data: Some(Tenant {
                    domain: Some("acme.dev".into()),
                    industry: Some("越权行业".into()),
                    remark: Some("越权备注".into()),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["domain".into()],
                }),
                allow_missing: None,
            },
        )
        .await
        .unwrap();

        let fresh = svc
            .get(
                me,
                GetTenantRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_tenant_request::QueryBy::Id(
                            acme_id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            fresh.domain.as_deref(),
            Some("acme.dev"),
            "masked field lands"
        );
        assert_eq!(
            fresh.industry, None,
            "unmasked industry is not writable through the gate"
        );
        assert_eq!(
            fresh.remark, None,
            "unmasked remark is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列；audit_status 的 UNSPECIFIED(0)
    /// 在掩码下仍不写列（审核态无清值语义）；无掩码 None 仍是"不动"。
    #[tokio::test]
    async fn masked_none_clears_nullable_tenant_column() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = admin_ctx();

        svc.create(
            me.clone(),
            CreateTenantRequest {
                data: Some(tenant("globex", Some(2))),
            },
        )
        .await
        .unwrap();
        let listed = svc
            .list(me.clone(), PagingRequest::default())
            .await
            .unwrap();
        let globex_id = listed.items[0].id.unwrap();

        // 无掩码：None 不动列（全量提交语义不回退）——remark 可写入但
        // 缺席字段不误清。
        svc.update(
            me.clone(),
            UpdateTenantRequest {
                id: globex_id,
                data: Some(Tenant {
                    remark: Some("待清备注".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = svc
            .get(
                me.clone(),
                GetTenantRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_tenant_request::QueryBy::Id(
                            globex_id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            fresh.remark.as_deref(),
            Some("待清备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 remark + audit_status：可空 remark 清列；audit_status
        // 载荷 0（UNSPECIFIED）照旧不写列，审核态保持 APPROVED。
        svc.update(
            me.clone(),
            UpdateTenantRequest {
                id: globex_id,
                data: Some(Tenant {
                    audit_status: Some(0),
                    ..Default::default()
                }),
                update_mask: Some(pbjson_types::FieldMask {
                    paths: vec!["remark".into(), "audit_status".into()],
                }),
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = svc
            .get(
                me,
                GetTenantRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_tenant_request::QueryBy::Id(
                            globex_id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.audit_status,
            Some(2),
            "UNSPECIFIED audit_status never writes under the mask"
        );
    }

    /// 平台专属写口（C4 补闸）：非平台操作者的 create/update/delete/
    /// cleanup_data 全部 FORBIDDEN——P2 收闸此前只收了读口，而 delete/
    /// cleanup_data 触发 cleanup_rows 级联清数，是破坏面最大的单口。
    #[tokio::test]
    async fn tenant_writes_require_platform_admin() {
        let state = test_state().await;
        let svc = TenantService {
            state: state.clone(),
        };
        let me = ctx(7, 1); // 非 ipa
        for err in [
            svc.create(
                me.clone(),
                CreateTenantRequest {
                    data: Some(tenant("acme9", None)),
                },
            )
            .await
            .unwrap_err()
            .reason,
            svc.update(
                me.clone(),
                UpdateTenantRequest {
                    id: 1,
                    data: Some(Tenant {
                        name: Some("越权改名".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err()
            .reason,
            svc.delete(
                me.clone(),
                DeleteTenantRequest {
                    query_by: Some(DeleteQueryBy::Id(1)),
                },
            )
            .await
            .unwrap_err()
            .reason,
            svc.cleanup_data(me.clone(), CleanupTenantDataRequest { id: 1 })
                .await
                .unwrap_err()
                .reason,
        ] {
            assert_eq!(err, "FORBIDDEN");
        }
        // 平台管理员照常通过（既有流程不回退）。
        svc.create(
            admin_ctx(),
            CreateTenantRequest {
                data: Some(tenant("acme10", None)),
            },
        )
        .await
        .unwrap();
    }
}
