//! UserService — service layer:
//! tenant-scoped user CRUD (list/get/create/update/delete/exists) plus the
//! forced password reset (EditUserPassword). Passwords arrive
//! base64(AES-CBC) like the login flow and are stored as bcrypt.

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};

use crate::admin::data::repos::UserRepo;
use crate::admin::data::Viewer;
use crate::admin::mapping;
use crate::admin::state::{
    db_err, internal_error, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use crate::proto::identity::service::v1::{
    CreateUserRequest, DeleteUserRequest, EditUserPasswordRequest, GetUserRequest,
    ListUserResponse, UpdateUserRequest, User, UserExistsRequest, UserExistsResponse,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

pub struct UserService {
    pub state: Arc<AppState>,
}

impl UserService {
    /// The viewer-driven repo: tenancy predicates live in the repo, not here.
    fn repo<'a>(&'a self, ctx: &rushwind_http_binding::ctx::RequestContext) -> UserRepo<'a> {
        UserRepo::new(&self.state.db, Viewer::from_ctx(ctx))
    }

    async fn find_by_username(
        &self,
        tenant_id: i64,
        username: &str,
    ) -> Result<Option<crate::admin::data::sys_users::Model>, StatusError> {
        crate::admin::data::sys_users::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_users::Column::TenantId.eq(tenant_id))
                    .add(crate::admin::data::sys_users::Column::Username.eq(username)),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)
    }

    /// The credential row create/update used by create + password edits.
    /// 传输形态统一入口（rsa-v1 信封优先，legacy AES 兼容），**明文兜底**：
    /// 参照基座的 prepareCredential 接受明文直传（创建用户表单的历史形态），
    /// 解密失败时原串按明文处理——两种前端形态都落到同一个 bcrypt 哈希。
    async fn upsert_credential<C>(
        db: &C,
        tenant_id: i64,
        user_id: i64,
        username: &str,
        encrypted_password: Option<&str>,
    ) -> Result<(), StatusError>
    where
        C: sea_orm::ConnectionTrait,
    {
        let Some(encrypted) = encrypted_password.filter(|v| !v.is_empty()) else {
            return Ok(());
        };
        let plain = match crate::admin::crypto::decrypt_transfer_password(encrypted) {
            Ok(plain) => plain,
            // 明文兜底：合法传输密文只有两种拼写，其余一律按明文收下
            //（基座同语义：创建路径不解密，直接对传入串做策略校验+哈希）。
            Err(_) => encrypted.to_string(),
        };
        let hash = crate::admin::crypto::hash_password(&plain).map_err(internal_error)?;
        let existing = crate::admin::data::sys_user_credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_credentials::Column::TenantId.eq(tenant_id))
                    .add(
                        crate::admin::data::sys_user_credentials::Column::IdentityType
                            .eq("USERNAME"),
                    )
                    .add(crate::admin::data::sys_user_credentials::Column::Identifier.eq(username)),
            )
            .one(db)
            .await
            .map_err(db_err)?;
        match existing {
            Some(row) => {
                let mut a: crate::admin::data::sys_user_credentials::ActiveModel = row.into();
                a.credential = Set(hash);
                a.updated_at = Set(Some(crate::admin::data::now()));
                a.update(db).await.map_err(db_err)?;
            }
            None => {
                crate::admin::data::sys_user_credentials::ActiveModel {
                    tenant_id: Set(Some(tenant_id)),
                    user_id: Set(Some(user_id)),
                    identity_type: Set(Some("USERNAME".into())),
                    identifier: Set(username.to_string()),
                    credential_type: Set(Some("PASSWORD_HASH".into())),
                    credential: Set(hash),
                    is_primary: Set(Some(true)),
                    status: Set(Some("ENABLED".into())),
                    created_at: Set(Some(crate::admin::data::now())),
                    updated_at: Set(Some(crate::admin::data::now())),
                    ..Default::default()
                }
                .insert(db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// The role binding for create/update (role_ids on the payload).
    /// 归属闸：先校验 role_ids 全部属于操作者租户再落绑定——绑定期不校验
    /// 时，载荷可以引用外租户（含平台侧）的角色 id，而 resolve_authority
    /// 解析权威只按 UserId 查绑定，下次登录/刷新即按外租户角色赋权（跨
    /// 租户提权入口）。重复 id 去重后计数比对。
    async fn sync_roles<C>(
        db: &C,
        tenant_id: i64,
        user_id: i64,
        role_ids: &[i64],
    ) -> Result<(), StatusError>
    where
        C: sea_orm::ConnectionTrait,
    {
        let unique: Vec<i64> = role_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if !unique.is_empty() {
            let owned = crate::admin::data::sys_roles::Entity::find()
                .filter(
                    Condition::all()
                        .add(crate::admin::data::sys_roles::Column::TenantId.eq(tenant_id))
                        .add(crate::admin::data::sys_roles::Column::Id.is_in(unique.clone())),
                )
                .count(db)
                .await
                .map_err(db_err)?;
            if owned as usize != unique.len() {
                return Err(status_error(
                    "BAD_REQUEST",
                    "role_ids contain a role outside the operator tenant",
                ));
            }
        }
        crate::admin::data::sys_user_roles::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_roles::Column::TenantId.eq(tenant_id))
                    .add(crate::admin::data::sys_user_roles::Column::UserId.eq(user_id)),
            )
            .exec(db)
            .await
            .map_err(db_err)?;
        for role_id in &unique {
            crate::admin::data::sys_user_roles::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                user_id: Set(Some(user_id)),
                role_id: Set(Some(*role_id)),
                status: Set(Some("ACTIVE".into())),
                assigned_at: Set(Some(crate::admin::data::now())),
                created_at: Set(Some(crate::admin::data::now())),
                updated_at: Set(Some(crate::admin::data::now())),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// 组织单元绑定（create/update 共用，delete-then-insert 整组重建）。
    /// 空列表 = 本次不动绑定（既不清也不建）；非空 = 清后全量重建，行全部
    /// status=ACTIVE / is_primary=true / assigned_at=start_at=now。
    async fn sync_org_units<C>(
        db: &C,
        tenant_id: i64,
        user_id: i64,
        org_unit_ids: &[i64],
        operator_id: i64,
    ) -> Result<(), StatusError>
    where
        C: sea_orm::ConnectionTrait,
    {
        let unique: Vec<i64> = org_unit_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if unique.is_empty() {
            return Ok(());
        }
        crate::admin::data::sys_user_org_units::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_org_units::Column::TenantId.eq(tenant_id))
                    .add(crate::admin::data::sys_user_org_units::Column::UserId.eq(user_id)),
            )
            .exec(db)
            .await
            .map_err(db_err)?;
        let now = crate::admin::data::now();
        for org_unit_id in &unique {
            crate::admin::data::sys_user_org_units::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                user_id: Set(Some(user_id)),
                org_unit_id: Set(Some(*org_unit_id)),
                is_primary: Set(Some(true)),
                status: Set(Some("ACTIVE".into())),
                assigned_at: Set(Some(now)),
                start_at: Set(Some(now)),
                created_by: Set(Some(operator_id)),
                created_at: Set(Some(now)),
                updated_at: Set(Some(now)),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// 岗位绑定（组织单元同款 delete-then-insert 语义）。
    async fn sync_positions<C>(
        db: &C,
        tenant_id: i64,
        user_id: i64,
        position_ids: &[i64],
        operator_id: i64,
    ) -> Result<(), StatusError>
    where
        C: sea_orm::ConnectionTrait,
    {
        let unique: Vec<i64> = position_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if unique.is_empty() {
            return Ok(());
        }
        crate::admin::data::sys_user_positions::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_positions::Column::TenantId.eq(tenant_id))
                    .add(crate::admin::data::sys_user_positions::Column::UserId.eq(user_id)),
            )
            .exec(db)
            .await
            .map_err(db_err)?;
        let now = crate::admin::data::now();
        for position_id in &unique {
            crate::admin::data::sys_user_positions::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                user_id: Set(Some(user_id)),
                position_id: Set(Some(*position_id)),
                is_primary: Set(Some(true)),
                status: Set(Some("ACTIVE".into())),
                assigned_at: Set(Some(now)),
                start_at: Set(Some(now)),
                created_by: Set(Some(operator_id)),
                created_at: Set(Some(now)),
                updated_at: Set(Some(now)),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// 关联 id 收集：单值并入复数集（>0 才收、重复去重）——参照的
    /// CreateWithTx/Update 收集语义（RoleId/OrgUnitId/PositionId 任一在
    /// mask 内即该类关联参与本次更新）。
    fn merge_relation_ids(plural: &[i64], singular: Option<i64>) -> Vec<i64> {
        let mut ids = plural.to_vec();
        if let Some(id) = singular.filter(|v| *v > 0) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// 字段写入统一过 update_mask 白名单门（字段级更新语义）：create 等
    /// 非掩码入口传 [`crate::fieldmask::FieldMask::unmasked`]，即存量全量
    /// 提交语义；update 传请求掩码，只有选中字段可动。
    fn apply_user_fields(
        a: &mut crate::data::sys_users::ActiveModel,
        data: &User,
        mask: &crate::fieldmask::FieldMask,
    ) {
        crate::set_masked!(req mask, a, username, data.username, |v: &String| v.clone());
        crate::set_masked!(opt mask, a, nickname, data.nickname, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, realname, data.realname, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, email, data.email, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, mobile, data.mobile, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, telephone, data.telephone, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, avatar, data.avatar, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, address, data.address, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, region, data.region, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, locale, data.locale, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, description, data.description, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
            v.clone()
        ));
        crate::set_masked!(opt mask, a, gender, data.gender, |v: &i32| Some(
            match v {
                1 => "MALE".to_string(),
                2 => "FEMALE".to_string(),
                _ => "SECRET".to_string(),
            }
        ));
        crate::set_masked!(opt mask, a, status, data.status, |v: &i32| Some(
            match v {
                1 => "NORMAL".to_string(),
                2 => "PENDING".to_string(),
                3 => "LOCKED".to_string(),
                4 => "EXPIRED".to_string(),
                9 => "CLOSED".to_string(),
                _ => "DISABLED".to_string(),
            }
        ));
        crate::set_masked!(opt mask, a, locked_until, data.locked_until, |v: &pbjson_types::Timestamp| {
            crate::state::ts_to_naive(v)
        });
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::UserServiceHandlers for UserService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListUserResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let (rows, total) = self.repo(&ctx).paged_list(tid, &req).await?;
        let items = load_user_views(&self.state, rows).await?;
        Ok(ListUserResponse {
            items,
            total: total as i64,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetUserRequest,
    ) -> Result<User, StatusError> {
        let payload = operator_of(&ctx)?;
        let tid = payload.tenant_id;
        let row = match req.query_by {
            Some(crate::proto::identity::service::v1::get_user_request::QueryBy::Id(id)) => {
                // Id 路径归属闸：原实现裸 id 全库找行（跨租户 PII 读取）。
                // 平台管理员放行，其余仅限本租户；拒绝按 not_found 应答。
                let found = crate::admin::data::sys_users::Entity::find_by_id(id)
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?;
                match found {
                    Some(r)
                        if payload.is_platform_admin == Some(true)
                            || r.tenant_id.unwrap_or(0) == tid =>
                    {
                        Some(r)
                    }
                    _ => None,
                }
            }
            Some(crate::proto::identity::service::v1::get_user_request::QueryBy::Username(
                name,
            )) => self.find_by_username(tid, &name).await?,
            None => None,
        }
        .ok_or_else(|| not_found("user"))?;
        let mut view = load_user_views(&self.state, vec![row]).await?;
        view.pop().ok_or_else(|| not_found("user"))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateUserRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::admin::state::require_data(req.data)?;
        let username = data.username.clone().unwrap_or_default();
        if username.is_empty() {
            return Err(status_error("BAD_REQUEST", "username required"));
        }
        if self
            .find_by_username(payload.tenant_id, &username)
            .await?
            .is_some()
        {
            return Err(status_error("BAD_REQUEST", "username already exists"));
        }
        let mut a = crate::admin::data::sys_users::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            username: Set(username.clone()),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        Self::apply_user_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked());
        crate::ensure_default!(a, status, "NORMAL".to_string());
        // 单值并入复数集（单值 >0 才收）。
        let role_ids = Self::merge_relation_ids(&data.role_ids, data.role_id);
        let org_unit_ids = Self::merge_relation_ids(&data.org_unit_ids, data.org_unit_id);
        let position_ids = Self::merge_relation_ids(&data.position_ids, data.position_id);
        // 单事务：用户行 + 凭证 + 角色/组织/岗位绑定原子落库——任一步失败
        // 即整体回滚，不再留下"提示错误但用户已创建"的半创建行。
        let txn = self.state.db.begin().await.map_err(db_err)?;
        let inserted = a.insert(&txn).await.map_err(db_err)?;
        Self::upsert_credential(
            &txn,
            payload.tenant_id,
            inserted.id,
            &username,
            req.password.as_deref(),
        )
        .await?;
        Self::sync_roles(&txn, payload.tenant_id, inserted.id, &role_ids).await?;
        Self::sync_org_units(
            &txn,
            payload.tenant_id,
            inserted.id,
            &org_unit_ids,
            payload.user_id,
        )
        .await?;
        Self::sync_positions(
            &txn,
            payload.tenant_id,
            inserted.id,
            &position_ids,
            payload.user_id,
        )
        .await?;
        txn.commit().await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateUserRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::admin::data::sys_users::Entity::find_by_id(req.id)
            .filter(crate::admin::data::sys_users::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user"))?;
        let mut a: crate::admin::data::sys_users::ActiveModel = row.into();
        let username_before = a.username.clone().unwrap().clone();
        let mut password_set = false;
        // 单事务：字段更新 + 角色绑定 + 密码凭据原子落库——原实现角色绑定
        // 跑在裸连接上，先删后插，中途失败用户角色被清空。
        let txn = self.state.db.begin().await.map_err(db_err)?;
        if let Some(data) = &req.data {
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。密码凭据是子资源写，
            // 保留原有触发条件不进门。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            Self::apply_user_fields(&mut a, data, &mask);
            crate::stamp_update!(a, payload.user_id);
            // 纯关系更新（掩码只选 *_ids，无标量列变化）合法：空 SET 的
            // RecordNotUpdated 不算错，放行到下面的关联重建。
            if let Err(e) = a.update(&txn).await {
                if !matches!(e, sea_orm::DbErr::RecordNotUpdated) {
                    return Err(db_err(e));
                }
            }
            // 关联（角色/组织/岗位）重建 = delete-then-insert 整组替换，
            // 只在对应字段确属本次更新范围（掩码选中或无掩码全量）时才
            // 收集——窄掩码的部分更新载荷里即便带了旧值也不得清空既有
            // 绑定。空集 = 本次不动绑定；非空 = 清后重建。
            let role_ids = if mask.touches("role_ids") || mask.touches("role_id") {
                Self::merge_relation_ids(&data.role_ids, data.role_id)
            } else {
                Vec::new()
            };
            let org_unit_ids = if mask.touches("org_unit_ids") || mask.touches("org_unit_id") {
                Self::merge_relation_ids(&data.org_unit_ids, data.org_unit_id)
            } else {
                Vec::new()
            };
            let position_ids = if mask.touches("position_ids") || mask.touches("position_id") {
                Self::merge_relation_ids(&data.position_ids, data.position_id)
            } else {
                Vec::new()
            };
            Self::sync_roles(&txn, payload.tenant_id, req.id, &role_ids).await?;
            Self::sync_org_units(
                &txn,
                payload.tenant_id,
                req.id,
                &org_unit_ids,
                payload.user_id,
            )
            .await?;
            Self::sync_positions(
                &txn,
                payload.tenant_id,
                req.id,
                &position_ids,
                payload.user_id,
            )
            .await?;
        }
        if req.password.as_deref().is_some_and(|v| !v.is_empty()) {
            Self::upsert_credential(
                &txn,
                payload.tenant_id,
                req.id,
                &username_before,
                req.password.as_deref(),
            )
            .await?;
            password_set = true;
        }
        txn.commit().await.map_err(db_err)?;
        if password_set {
            self.state.tokens.revoke_user_token(req.id).await;
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteUserRequest,
    ) -> Result<Empty, StatusError> {
        let tid = tenant_of(&ctx);
        let id = match req.query_by {
            Some(crate::proto::identity::service::v1::delete_user_request::QueryBy::Id(id)) => id,
            Some(crate::proto::identity::service::v1::delete_user_request::QueryBy::Username(
                name,
            )) => self
                .find_by_username(tid, &name)
                .await?
                .map(|r| r.id)
                .ok_or_else(|| not_found("user"))?,
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        };
        // Id 路径归属闸：原实现 delete_by_id 直删任意租户的用户（跨租户
        // 删号 + 平台侧账号删除）。平台管理员放行，其余仅限本租户；拒绝
        // 按 not_found 应答（防枚举）。
        let row = crate::admin::data::sys_users::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user"))?;
        let row_tid = row.tenant_id.unwrap_or(0);
        let payload = operator_of(&ctx)?;
        let is_platform = payload.is_platform_admin == Some(true);
        if !is_platform && row_tid != payload.tenant_id {
            return Err(not_found("user"));
        }
        // 单事务：凭证 + 角色/组织/岗位绑定 + 用户行原子清除。
        let txn = self.state.db.begin().await.map_err(db_err)?;
        crate::admin::data::sys_user_credentials::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_credentials::Column::TenantId.eq(row_tid))
                    .add(crate::admin::data::sys_user_credentials::Column::UserId.eq(id)),
            )
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_user_roles::Entity::delete_many()
            .filter(crate::admin::data::sys_user_roles::Column::UserId.eq(id))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_user_org_units::Entity::delete_many()
            .filter(crate::admin::data::sys_user_org_units::Column::UserId.eq(id))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_user_positions::Entity::delete_many()
            .filter(crate::admin::data::sys_user_positions::Column::UserId.eq(id))
            .exec(&txn)
            .await
            .map_err(db_err)?;
        crate::admin::data::sys_users::Entity::delete_by_id(id)
            .exec(&txn)
            .await
            .map_err(db_err)?;
        txn.commit().await.map_err(db_err)?;
        self.state.tokens.revoke_user_token(id).await;
        Ok(Empty {})
    }

    async fn user_exists(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UserExistsRequest,
    ) -> Result<UserExistsResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let exist = match req.query_by {
            Some(crate::proto::identity::service::v1::user_exists_request::QueryBy::Id(id)) => {
                // Id 路径归属闸：跨租户 id 不报存在（防存在性探测），平台
                // 管理员放行。
                let payload = operator_of(&ctx)?;
                let found = crate::admin::data::sys_users::Entity::find_by_id(id)
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?;
                matches!(
                    found,
                    Some(r)
                        if payload.is_platform_admin == Some(true)
                            || r.tenant_id.unwrap_or(0) == tid
                )
            }
            Some(crate::proto::identity::service::v1::user_exists_request::QueryBy::Username(
                name,
            )) => self.find_by_username(tid, &name).await?.is_some(),
            None => false,
        };
        Ok(UserExistsResponse { exist })
    }

    async fn edit_user_password(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: EditUserPasswordRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        // 归属闸：本人改自己、平台管理员代改、或同租户内的操作（租户内部
        // 的细粒度权限仍归授权引擎；跨租户与对平台侧账号的改动一律关死）。
        // 原实现无任何校验——任意登录用户可重置任意 user_id 的密码并吊销
        // 其全部会话（任意账户接管）。
        let target = crate::admin::data::sys_users::Entity::find_by_id(req.user_id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user"))?;
        let target_tid = target.tenant_id.unwrap_or(0);
        let allowed = payload.user_id == target.id
            || payload.is_platform_admin == Some(true)
            || (target_tid != 0 && target_tid == payload.tenant_id);
        if !allowed {
            return Err(status_error(
                "FORBIDDEN",
                "cannot edit a password outside the operator tenant",
            ));
        }
        // Plaintext over the wire here (the proto validates 8..=128);
        // complexity mirrors the policy checks.
        let min_len = crate::admin::data::sys_configs::Entity::find()
            .filter(crate::admin::data::sys_configs::Column::Key.eq("sys.password.minLen"))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .and_then(|r| r.value)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(8);
        if (req.new_password.len() as i64) < min_len {
            return Err(status_error(
                "BAD_REQUEST",
                "password does not meet complexity requirements",
            ));
        }
        let hash =
            crate::admin::crypto::hash_password(&req.new_password).map_err(internal_error)?;
        // 凭证定位带租户谓词：原实现只按 (USERNAME, user_id) 全库找行。
        let cred = crate::admin::data::sys_user_credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::admin::data::sys_user_credentials::Column::TenantId.eq(target_tid))
                    .add(
                        crate::admin::data::sys_user_credentials::Column::IdentityType
                            .eq("USERNAME"),
                    )
                    .add(crate::admin::data::sys_user_credentials::Column::UserId.eq(req.user_id)),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user credential"))?;
        let mut a: crate::admin::data::sys_user_credentials::ActiveModel = cred.into();
        a.credential = Set(hash);
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        self.state.tokens.revoke_user_token(req.user_id).await;
        Ok(Empty {})
    }
}

/// Loads the operator's user row with role codes resolved (the `/me`
/// shape).
pub(crate) async fn load_user(
    state: &AppState,
    uid: i64,
) -> Result<(crate::admin::data::sys_users::Model, Vec<String>), crate::admin::state::StatusError> {
    let user = crate::admin::data::sys_users::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(|e| internal_error(format!("db: {e}")))?
        .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
    let role_ids: Vec<i64> = crate::admin::data::sys_user_roles::Entity::find()
        .filter(crate::admin::data::sys_user_roles::Column::UserId.eq(uid))
        .all(&state.db)
        .await
        .map_err(|e| internal_error(format!("db: {e}")))?
        .iter()
        .filter_map(|r| r.role_id)
        .collect();
    let codes = if role_ids.is_empty() {
        Vec::new()
    } else {
        crate::admin::data::sys_roles::Entity::find()
            .filter(crate::admin::data::sys_roles::Column::Id.is_in(role_ids))
            .all(&state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .into_iter()
            .map(|r| r.code)
            .collect()
    };
    Ok((user, codes))
}

/// 用户响应的绑定面：绑定 id 集 + 名称/编码回填（用户视图装配的产物）。
pub(crate) struct UserBindings {
    pub role_ids: Vec<i64>,
    pub role_codes: Vec<String>,
    pub role_names: Vec<String>,
    pub org_unit_ids: Vec<i64>,
    pub org_unit_names: Vec<String>,
    pub position_ids: Vec<i64>,
    pub position_names: Vec<String>,
    pub tenant_name: Option<String>,
}

impl UserBindings {
    /// 仅角色编码面（portal/profile 视图沿用既有线形态，组织/岗位留空）。
    pub(crate) fn roles_only(codes: Vec<String>) -> Self {
        Self {
            role_ids: Vec::new(),
            role_codes: codes,
            role_names: Vec::new(),
            org_unit_ids: Vec::new(),
            org_unit_names: Vec::new(),
            position_ids: Vec::new(),
            position_names: Vec::new(),
            tenant_name: None,
        }
    }
}

/// User row → proto User (the identity fields; the binding ids ride their
/// `*_ids` lists, names ride the `*_names` lists, role codes the `roles`
/// list).
pub(crate) fn user_to_proto(
    user: crate::admin::data::sys_users::Model,
    bindings: UserBindings,
) -> User {
    let UserBindings {
        role_ids,
        role_codes,
        role_names,
        org_unit_ids,
        org_unit_names,
        position_ids,
        position_names,
        tenant_name,
    } = bindings;
    User {
        home_path: None,
        id: Some(user.id),
        tenant_id: user.tenant_id,
        username: Some(user.username),
        nickname: user.nickname,
        realname: user.realname,
        avatar: user.avatar,
        email: user.email,
        mobile: user.mobile,
        telephone: user.telephone,
        gender: user
            .gender
            .as_deref()
            .map(|g| mapping::user_gender_of(g).unwrap_or(0)),
        address: user.address,
        region: user.region,
        locale: user.locale,
        description: user.description,
        last_login_at: user
            .last_login_at
            .and_then(crate::admin::state::naive_to_ts),
        last_login_ip: user.last_login_ip,
        status: user
            .status
            .as_deref()
            .map(|s| mapping::user_status_of(s).unwrap_or(0)),
        locked_until: user.locked_until.and_then(crate::admin::state::naive_to_ts),
        created_by: user.created_by,
        updated_by: user.updated_by,
        deleted_by: user.deleted_by,
        created_at: user.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: user.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: user.deleted_at.and_then(crate::admin::state::naive_to_ts),
        tenant_name,
        org_unit_id: None,
        org_unit_ids,
        org_unit_name: None,
        org_unit_names,
        position_id: None,
        position_ids,
        position_name: None,
        position_names,
        role_id: None,
        role_ids,
        roles: role_codes,
        role_names,
        remark: user.remark,
    }
}

/// 用户视图装配：绑定 id 集（角色/组织/岗位）按用户批量加载，名称与编码
/// 按去重后的 id 集各一把查回——Get 单条与 List 批量共用（对位参照的
/// enrichRelations 聚合面：列表把 N+1 压平成关系 3 查 + 名称 4 查）。
pub(crate) async fn load_user_views(
    state: &Arc<AppState>,
    rows: Vec<crate::admin::data::sys_users::Model>,
) -> Result<Vec<User>, StatusError> {
    use std::collections::{BTreeMap, BTreeSet};

    let user_ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut role_of: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut org_of: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut pos_of: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    if !user_ids.is_empty() {
        let role_rows = crate::admin::data::sys_user_roles::Entity::find()
            .filter(crate::admin::data::sys_user_roles::Column::UserId.is_in(&user_ids))
            .order_by_asc(crate::admin::data::sys_user_roles::Column::Id)
            .all(&state.db)
            .await
            .map_err(db_err)?;
        for r in role_rows
            .into_iter()
            .filter_map(|r| match (r.user_id, r.role_id) {
                (Some(u), Some(v)) => Some((u, v)),
                _ => None,
            })
        {
            role_of.entry(r.0).or_default().push(r.1);
        }
        let org_rows = crate::admin::data::sys_user_org_units::Entity::find()
            .filter(crate::admin::data::sys_user_org_units::Column::UserId.is_in(&user_ids))
            .order_by_asc(crate::admin::data::sys_user_org_units::Column::Id)
            .all(&state.db)
            .await
            .map_err(db_err)?;
        for r in org_rows
            .into_iter()
            .filter_map(|r| match (r.user_id, r.org_unit_id) {
                (Some(u), Some(v)) => Some((u, v)),
                _ => None,
            })
        {
            org_of.entry(r.0).or_default().push(r.1);
        }
        let pos_rows = crate::admin::data::sys_user_positions::Entity::find()
            .filter(crate::admin::data::sys_user_positions::Column::UserId.is_in(&user_ids))
            .order_by_asc(crate::admin::data::sys_user_positions::Column::Id)
            .all(&state.db)
            .await
            .map_err(db_err)?;
        for r in pos_rows
            .into_iter()
            .filter_map(|r| match (r.user_id, r.position_id) {
                (Some(u), Some(v)) => Some((u, v)),
                _ => None,
            })
        {
            pos_of.entry(r.0).or_default().push(r.1);
        }
    }

    // 名称/编码回填：各面涉及 id 去重后按 id IN 一把查回（绑定 id 引用
    // 已删除行时名字静默缺席，与参照一致）。
    let role_ids: BTreeSet<i64> = role_of.values().flatten().copied().collect();
    let roles: BTreeMap<i64, (String, String)> = if role_ids.is_empty() {
        BTreeMap::new()
    } else {
        crate::admin::data::sys_roles::Entity::find()
            .filter(crate::admin::data::sys_roles::Column::Id.is_in(role_ids))
            .all(&state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, (r.name, r.code)))
            .collect()
    };
    let org_ids: BTreeSet<i64> = org_of.values().flatten().copied().collect();
    let orgs: BTreeMap<i64, String> = if org_ids.is_empty() {
        BTreeMap::new()
    } else {
        crate::admin::data::sys_org_units::Entity::find()
            .filter(crate::admin::data::sys_org_units::Column::Id.is_in(org_ids))
            .all(&state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect()
    };
    let pos_ids: BTreeSet<i64> = pos_of.values().flatten().copied().collect();
    let positions: BTreeMap<i64, String> = if pos_ids.is_empty() {
        BTreeMap::new()
    } else {
        crate::admin::data::sys_positions::Entity::find()
            .filter(crate::admin::data::sys_positions::Column::Id.is_in(pos_ids))
            .all(&state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect()
    };
    let tenant_ids: BTreeSet<i64> = rows.iter().filter_map(|r| r.tenant_id).collect();
    let tenants: BTreeMap<i64, String> = if tenant_ids.is_empty() {
        BTreeMap::new()
    } else {
        crate::admin::data::sys_tenants::Entity::find()
            .filter(crate::admin::data::sys_tenants::Column::Id.is_in(tenant_ids))
            .all(&state.db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect()
    };

    Ok(rows
        .into_iter()
        .map(|user| {
            let role_ids = role_of.remove(&user.id).unwrap_or_default();
            let role_codes: Vec<String> = role_ids
                .iter()
                .filter_map(|id| roles.get(id).map(|(_, code)| code.clone()))
                .collect();
            let role_names: Vec<String> = role_ids
                .iter()
                .filter_map(|id| roles.get(id).map(|(name, _)| name.clone()))
                .collect();
            let org_unit_ids = org_of.remove(&user.id).unwrap_or_default();
            let org_unit_names: Vec<String> = org_unit_ids
                .iter()
                .filter_map(|id| orgs.get(id).cloned())
                .collect();
            let position_ids = pos_of.remove(&user.id).unwrap_or_default();
            let position_names: Vec<String> = position_ids
                .iter()
                .filter_map(|id| positions.get(id).cloned())
                .collect();
            let tenant_name = user.tenant_id.and_then(|tid| tenants.get(&tid).cloned());
            user_to_proto(
                user,
                UserBindings {
                    role_ids,
                    role_codes,
                    role_names,
                    org_unit_ids,
                    org_unit_names,
                    position_ids,
                    position_names,
                    tenant_name,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_users as users;
    use crate::proto::gen::services::UserServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, username: &str) -> users::Model {
        users::ActiveModel {
            tenant_id: Set(Some(1)),
            username: Set(username.into()),
            nickname: Set(Some("旧昵称".into())),
            mobile: Set(Some("13800000000".into())),
            status: Set(Some("NORMAL".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> users::Model {
        users::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: i64, data: User, paths: &[&str]) -> UpdateUserRequest {
        UpdateUserRequest {
            id,
            data: Some(data),
            password: None,
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门：掩码只选 email 时，载荷里多带的 mobile 与
    /// nickname 不许落列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let me = ctx(1, 9);
        let db = &state.db;
        let row = plant(db, "alice").await;

        svc.update(
            me,
            update_req(
                row.id,
                User {
                    email: Some("alice@corp.dev".into()),
                    mobile: Some("19999999999".into()),
                    nickname: Some("越权昵称".into()),
                    ..Default::default()
                },
                &["email"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.email.as_deref(),
            Some("alice@corp.dev"),
            "masked field lands"
        );
        assert_eq!(
            fresh.mobile.as_deref(),
            Some("13800000000"),
            "unmasked mobile is not writable through the gate"
        );
        assert_eq!(
            fresh.nickname.as_deref(),
            Some("旧昵称"),
            "unmasked nickname is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（可空列清值通道）；NOT NULL 列
    /// （username）同样选中时跳过不清；无掩码 None 仍是"不动"。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "bob").await;

        // 无掩码：None 不动列（全量提交语义不回退）。
        svc.update(
            ctx(1, 9),
            UpdateUserRequest {
                id: row.id,
                data: Some(User {
                    nickname: Some("新昵称".into()),
                    ..Default::default()
                }),
                password: None,
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.nickname.as_deref(), Some("新昵称"));
        assert_eq!(
            fresh.mobile.as_deref(),
            Some("13800000000"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 username + mobile、载荷全 None：可空 mobile 清列，
        // NOT NULL username 原样保留。
        svc.update(
            ctx(1, 9),
            update_req(row.id, User::default(), &["username", "mobile"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.mobile.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(fresh.username, "bob", "NOT NULL column never clears");
    }

    /// create 省略 optional 字段（status）不得 panic（ensure_default! 回归）：
    /// 缺省路径正是原 `a.status.clone().unwrap()` 炸 NotSet 的地方——载荷
    /// 不带 status 时该值保持 NotSet，unwrap 在"补默认值"的本意路径上把
    /// handler 炸掉。
    #[tokio::test]
    async fn create_without_optional_fields_defaults_status() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("carol".into()),
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .expect("omitted optional fields must not panic");
        let row = users::Entity::find()
            .filter(users::Column::Username.eq("carol"))
            .one(&state.db)
            .await
            .unwrap()
            .expect("user row");
        assert_eq!(row.status.as_deref(), Some("NORMAL"), "default status");
    }

    /// 角色绑定归属闸（C1）：role_ids 引用外租户（平台侧）角色 →
    /// BAD_REQUEST，且单事务回滚不留半创建用户行——原实现盲插任意
    /// role_id，resolve_authority 只按 UserId 查绑定，下次登录即按平台
    /// 角色赋权（跨租户提权）。
    #[tokio::test]
    async fn create_rejects_foreign_tenant_roles() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let platform_role = crate::admin::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(0)),
            name: Set("平台管理员".into()),
            code: Set("platform:admin".into()),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();

        let err = svc
            .create(
                ctx(1, 9),
                CreateUserRequest {
                    data: Some(User {
                        username: Some("mallory".into()),
                        role_ids: vec![platform_role.id],
                        ..Default::default()
                    }),
                    password: None,
                },
            )
            .await
            .expect_err("foreign tenant role must be rejected");
        assert_eq!(err.reason, "BAD_REQUEST", "ownership gate fires");
        let row = users::Entity::find()
            .filter(users::Column::Username.eq("mallory"))
            .one(&state.db)
            .await
            .unwrap();
        assert!(
            row.is_none(),
            "transaction rolled back, no half-created row"
        );
    }

    /// edit_user_password 归属闸（C2）：跨租户目标 FORBIDDEN，同租户放行，
    /// 平台管理员全域——原实现无任何校验，任意登录用户可重置任意 user_id
    /// 的密码并吊销其全部会话。
    #[tokio::test]
    async fn edit_user_password_is_tenant_gated() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let platform_user = users::ActiveModel {
            tenant_id: Set(Some(0)),
            username: Set("root".into()),
            status: Set(Some("NORMAL".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        plant_credential(db, 0, platform_user.id, "root").await;
        let target = plant(db, "bob").await;
        plant_credential(db, 1, target.id, "bob").await;

        // 跨租户：租户 1 操作者改平台用户 → FORBIDDEN。
        let err = svc
            .edit_user_password(
                ctx(1, 9),
                EditUserPasswordRequest {
                    user_id: platform_user.id,
                    new_password: "Str0ngPass!".into(),
                },
            )
            .await
            .expect_err("cross-tenant password edit must be rejected");
        assert_eq!(err.reason, "FORBIDDEN");

        // 同租户：放行（租户内细粒度权限归授权引擎）。
        svc.edit_user_password(
            ctx(1, 9),
            EditUserPasswordRequest {
                user_id: target.id,
                new_password: "Str0ngPass!".into(),
            },
        )
        .await
        .expect("same-tenant password edit passes");

        // 平台管理员：全域放行。
        let mut admin = ctx(0, 1);
        if let Some(claims) = admin.claims.as_mut() {
            claims.insert("ipa".into(), serde_json::json!(true));
        }
        svc.edit_user_password(
            admin,
            EditUserPasswordRequest {
                user_id: platform_user.id,
                new_password: "Str0ngPass!".into(),
            },
        )
        .await
        .expect("platform admin passes");
    }

    /// get 的 Id 路径归属闸（C3）：跨租户 id 按 not_found 应答（防 PII
    /// 读取与枚举），本租户正常返回，平台管理员全域——原实现裸 id 全库
    /// 找行。
    #[tokio::test]
    async fn get_id_path_is_tenant_scoped() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let mine = plant(db, "alice").await;
        let foreign = users::ActiveModel {
            tenant_id: Set(Some(2)),
            username: Set("eve".into()),
            status: Set(Some("NORMAL".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let u = svc
            .get(
                ctx(1, 9),
                GetUserRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_user_request::QueryBy::Id(mine.id),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(u.username, Some("alice".into()));

        let err = svc
            .get(
                ctx(1, 9),
                GetUserRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_user_request::QueryBy::Id(
                            foreign.id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .expect_err("cross-tenant id must not resolve");
        assert_eq!(err.reason, "NOT_FOUND");

        let mut admin = ctx(0, 1);
        if let Some(claims) = admin.claims.as_mut() {
            claims.insert("ipa".into(), serde_json::json!(true));
        }
        let u = svc
            .get(
                admin,
                GetUserRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_user_request::QueryBy::Id(
                            foreign.id,
                        ),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(u.username, Some("eve".into()));
    }

    /// 凭证行种子（edit_user_password 的定位前提）。
    async fn plant_credential(
        db: &sea_orm::DatabaseConnection,
        tenant_id: i64,
        user_id: i64,
        username: &str,
    ) {
        crate::admin::data::sys_user_credentials::ActiveModel {
            tenant_id: Set(Some(tenant_id)),
            user_id: Set(Some(user_id)),
            identity_type: Set(Some("USERNAME".into())),
            identifier: Set(username.into()),
            credential_type: Set(Some("PASSWORD_HASH".into())),
            credential: Set("seed".into()),
            is_primary: Set(Some(true)),
            status: Set(Some("ENABLED".into())),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn plant_org(
        db: &sea_orm::DatabaseConnection,
        tenant_id: i64,
        name: &str,
    ) -> crate::admin::data::sys_org_units::Model {
        crate::admin::data::sys_org_units::ActiveModel {
            tenant_id: Set(Some(tenant_id)),
            name: Set(name.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn plant_position(
        db: &sea_orm::DatabaseConnection,
        tenant_id: i64,
        name: &str,
    ) -> crate::admin::data::sys_positions::Model {
        crate::admin::data::sys_positions::ActiveModel {
            tenant_id: Set(Some(tenant_id)),
            name: Set(name.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn org_bindings_of(db: &sea_orm::DatabaseConnection, user_id: i64) -> Vec<i64> {
        crate::admin::data::sys_user_org_units::Entity::find()
            .filter(crate::admin::data::sys_user_org_units::Column::UserId.eq(user_id))
            .all(db)
            .await
            .unwrap()
            .iter()
            .filter_map(|r| r.org_unit_id)
            .collect()
    }

    async fn position_bindings_of(db: &sea_orm::DatabaseConnection, user_id: i64) -> Vec<i64> {
        crate::admin::data::sys_user_positions::Entity::find()
            .filter(crate::admin::data::sys_user_positions::Column::UserId.eq(user_id))
            .all(db)
            .await
            .unwrap()
            .iter()
            .filter_map(|r| r.position_id)
            .collect()
    }

    /// create：载荷里的 orgUnitIds/positionIds 落绑定表（此前整类关联被
    /// 静默丢弃——用户-组织关联从未持久化），单值 orgUnitId 并入复数集。
    #[tokio::test]
    async fn create_persists_org_and_position_bindings() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org_a = plant_org(db, 1, "总部").await;
        let org_b = plant_org(db, 1, "研发部").await;
        let position = plant_position(db, 1, "工程师").await;

        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("alice".into()),
                    org_unit_ids: vec![org_a.id],
                    org_unit_id: Some(org_b.id),
                    position_ids: vec![position.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();

        let user = users::Entity::find()
            .filter(users::Column::Username.eq("alice"))
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let mut orgs = org_bindings_of(db, user.id).await;
        orgs.sort();
        assert_eq!(orgs, vec![org_a.id, org_b.id], "singular merged into set");
        assert_eq!(position_bindings_of(db, user.id).await, vec![position.id]);
    }

    /// get：绑定 id 集与名称面（orgUnitNames/positionNames/roleIds/roles/
    /// roleNames）随响应回填——此前 org/position 域恒空。
    #[tokio::test]
    async fn get_backfills_binding_ids_and_names() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org = plant_org(db, 1, "财务部").await;
        let position = plant_position(db, 1, "主管").await;
        let role = crate::admin::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set("会计角色".into()),
            code: Set("tenant:accountant".into()),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("carol".into()),
                    org_unit_ids: vec![org.id],
                    position_ids: vec![position.id],
                    role_ids: vec![role.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();
        let user = users::Entity::find()
            .filter(users::Column::Username.eq("carol"))
            .one(db)
            .await
            .unwrap()
            .unwrap();

        let u = svc
            .get(
                ctx(1, 9),
                GetUserRequest {
                    query_by: Some(
                        crate::proto::identity::service::v1::get_user_request::QueryBy::Id(user.id),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(u.org_unit_ids, vec![org.id]);
        assert_eq!(u.org_unit_names, vec!["财务部".to_string()]);
        assert_eq!(u.position_ids, vec![position.id]);
        assert_eq!(u.position_names, vec!["主管".to_string()]);
        assert_eq!(u.role_ids, vec![role.id]);
        assert_eq!(u.roles, vec!["tenant:accountant".to_string()]);
        assert_eq!(u.role_names, vec!["会计角色".to_string()]);
    }

    /// 窄掩码的部分更新：载荷里带空 orgUnitIds 不得清空既有绑定
    /// （关联重建只在对应字段确属本次更新范围时收集）。
    #[tokio::test]
    async fn narrow_mask_update_keeps_existing_bindings() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org = plant_org(db, 1, "运营部").await;
        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("dave".into()),
                    org_unit_ids: vec![org.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();
        let user = users::Entity::find()
            .filter(users::Column::Username.eq("dave"))
            .one(db)
            .await
            .unwrap()
            .unwrap();

        svc.update(
            ctx(1, 9),
            update_req(
                user.id,
                User {
                    nickname: Some("新昵称".into()),
                    org_unit_ids: vec![], // 编辑抽屉的全量表单形态：旧值/空值随载荷
                    ..Default::default()
                },
                &["nickname"],
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            org_bindings_of(db, user.id).await,
            vec![org.id],
            "narrow mask must not clobber bindings"
        );
    }

    /// 掩码选中 org_unit_ids：非空列表 = 整组替换；空列表 = 本次不动
    /// （参照 AssignUserOrgUnits 的空集早退——空数组不清空既有绑定）。
    #[tokio::test]
    async fn masked_org_update_replaces_or_keeps_set() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org_a = plant_org(db, 1, "A").await;
        let org_b = plant_org(db, 1, "B").await;
        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("erin".into()),
                    org_unit_ids: vec![org_a.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();
        let user = users::Entity::find()
            .filter(users::Column::Username.eq("erin"))
            .one(db)
            .await
            .unwrap()
            .unwrap();

        // 非空替换。
        svc.update(
            ctx(1, 9),
            update_req(
                user.id,
                User {
                    org_unit_ids: vec![org_b.id],
                    ..Default::default()
                },
                &["org_unit_ids"],
            ),
        )
        .await
        .unwrap();
        assert_eq!(org_bindings_of(db, user.id).await, vec![org_b.id]);

        // 空列表 = 不动（不清空）。
        svc.update(
            ctx(1, 9),
            update_req(
                user.id,
                User {
                    org_unit_ids: vec![],
                    ..Default::default()
                },
                &["org_unit_ids"],
            ),
        )
        .await
        .unwrap();
        assert_eq!(org_bindings_of(db, user.id).await, vec![org_b.id]);
    }

    /// delete：凭证 + 角色/组织/岗位绑定 + 用户行一起清（参照
    /// removeUserRelations 三面全清）。
    #[tokio::test]
    async fn delete_cleans_all_relation_rows() {
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org = plant_org(db, 1, "临时部").await;
        let position = plant_position(db, 1, "临时岗").await;
        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("frank".into()),
                    org_unit_ids: vec![org.id],
                    position_ids: vec![position.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();
        let user = users::Entity::find()
            .filter(users::Column::Username.eq("frank"))
            .one(db)
            .await
            .unwrap()
            .unwrap();

        svc.delete(
            ctx(1, 9),
            DeleteUserRequest {
                query_by: Some(
                    crate::proto::identity::service::v1::delete_user_request::QueryBy::Id(user.id),
                ),
            },
        )
        .await
        .unwrap();
        assert!(
            org_bindings_of(db, user.id).await.is_empty(),
            "org bindings wiped with the user"
        );
        assert!(
            position_bindings_of(db, user.id).await.is_empty(),
            "position bindings wiped with the user"
        );
    }

    /// 分页 query 关系键过滤：org_unit_id 命中绑定该组织的用户；键从通用
    /// 过滤管线摘除（此前该键直接落列绑定拼出不存在的列）。
    #[tokio::test]
    async fn list_filters_users_by_org_unit_query_key() {
        use crate::proto::pagination::paging_request::FilteringType;
        let state = test_state().await;
        let svc = UserService {
            state: state.clone(),
        };
        let db = &state.db;
        let org = plant_org(db, 1, "命中部").await;
        plant(db, "gina").await;
        svc.create(
            ctx(1, 9),
            CreateUserRequest {
                data: Some(User {
                    username: Some("hunter".into()),
                    org_unit_ids: vec![org.id],
                    ..Default::default()
                }),
                password: None,
            },
        )
        .await
        .unwrap();

        let req = PagingRequest {
            filtering_type: Some(FilteringType::Query(r#"{"org_unit_id":"1"}"#.into())),
            ..Default::default()
        };
        let resp = svc.list(ctx(1, 9), req).await.unwrap();
        let names: Vec<String> = resp
            .items
            .iter()
            .filter_map(|u| u.username.clone())
            .collect();
        assert_eq!(names, vec!["hunter".to_string()]);
        assert_eq!(resp.total, 1);
    }
}
