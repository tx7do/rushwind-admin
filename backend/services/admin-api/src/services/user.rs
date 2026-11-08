//! UserService — service layer:
//! tenant-scoped user CRUD (list/get/create/update/delete/exists) plus the
//! forced password reset (EditUserPassword). Passwords arrive
//! base64(AES-CBC) like the login flow and are stored as bcrypt.

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::data::repos::UserRepo;
use crate::data::Viewer;
use crate::mapping;
use crate::state::{
    db_err, internal_error, not_found, operator_of, status_error, tenant_of, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::identity::service::v1::{
    CreateUserRequest, DeleteUserRequest, EditUserPasswordRequest, GetUserRequest,
    ListUserResponse, UpdateUserRequest, User, UserExistsRequest, UserExistsResponse,
};
use proto::proto::pagination::PagingRequest;

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
        tenant_id: u32,
        username: &str,
    ) -> Result<Option<crate::data::sys_users::Model>, StatusError> {
        crate::data::sys_users::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_users::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_users::Column::Username.eq(username)),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)
    }

    /// The credential row create/update used by create + password edits.
    async fn upsert_credential(
        &self,
        tenant_id: u32,
        user_id: u32,
        username: &str,
        encrypted_password: Option<&str>,
    ) -> Result<(), StatusError> {
        let Some(encrypted) = encrypted_password.filter(|v| !v.is_empty()) else {
            return Ok(());
        };
        let plain = crate::crypto::decrypt_transfer_password(encrypted)
            .map_err(|_| status_error("BAD_REQUEST", "invalid credential format"))?;
        let hash = crate::crypto::hash_password(&plain).map_err(internal_error)?;
        let existing = crate::data::sys_user_credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_credentials::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_user_credentials::Column::IdentityType.eq("USERNAME"))
                    .add(crate::data::sys_user_credentials::Column::Identifier.eq(username)),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)?;
        match existing {
            Some(row) => {
                let mut a: crate::data::sys_user_credentials::ActiveModel = row.into();
                a.credential = Set(hash);
                a.updated_at = Set(Some(crate::data::now()));
                a.update(&self.state.db).await.map_err(db_err)?;
            }
            None => {
                crate::data::sys_user_credentials::ActiveModel {
                    tenant_id: Set(Some(tenant_id)),
                    user_id: Set(Some(user_id)),
                    identity_type: Set(Some("USERNAME".into())),
                    identifier: Set(username.to_string()),
                    credential_type: Set(Some("PASSWORD_HASH".into())),
                    credential: Set(hash),
                    is_primary: Set(Some(true)),
                    status: Set(Some("ENABLED".into())),
                    created_at: Set(Some(crate::data::now())),
                    updated_at: Set(Some(crate::data::now())),
                    ..Default::default()
                }
                .insert(&self.state.db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// The role binding for create/update (role_ids on the payload).
    async fn sync_roles(
        &self,
        tenant_id: u32,
        user_id: u32,
        role_ids: &[u32],
    ) -> Result<(), StatusError> {
        crate::data::sys_user_roles::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_roles::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_user_roles::Column::UserId.eq(user_id)),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        for role_id in role_ids {
            crate::data::sys_user_roles::ActiveModel {
                tenant_id: Set(Some(tenant_id)),
                user_id: Set(Some(user_id)),
                role_id: Set(Some(*role_id)),
                status: Set(Some("ACTIVE".into())),
                assigned_at: Set(Some(crate::data::now())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }
            .insert(&self.state.db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    fn apply_user_fields(a: &mut crate::data::sys_users::ActiveModel, data: &User) {
        if let Some(v) = &data.username {
            a.username = Set(v.clone());
        }
        if let Some(v) = &data.nickname {
            a.nickname = Set(Some(v.clone()));
        }
        if let Some(v) = &data.realname {
            a.realname = Set(Some(v.clone()));
        }
        if let Some(v) = &data.email {
            a.email = Set(Some(v.clone()));
        }
        if let Some(v) = &data.mobile {
            a.mobile = Set(Some(v.clone()));
        }
        if let Some(v) = &data.telephone {
            a.telephone = Set(Some(v.clone()));
        }
        if let Some(v) = &data.avatar {
            a.avatar = Set(Some(v.clone()));
        }
        if let Some(v) = &data.address {
            a.address = Set(Some(v.clone()));
        }
        if let Some(v) = &data.region {
            a.region = Set(Some(v.clone()));
        }
        if let Some(v) = &data.locale {
            a.locale = Set(Some(v.clone()));
        }
        if let Some(v) = &data.description {
            a.description = Set(Some(v.clone()));
        }
        if let Some(v) = &data.remark {
            a.remark = Set(Some(v.clone()));
        }
        if let Some(v) = data.gender {
            a.gender = Set(Some(match v {
                1 => "MALE".to_string(),
                2 => "FEMALE".to_string(),
                _ => "SECRET".to_string(),
            }));
        }
        if let Some(v) = data.status {
            a.status = Set(Some(match v {
                1 => "NORMAL".to_string(),
                2 => "PENDING".to_string(),
                3 => "LOCKED".to_string(),
                4 => "EXPIRED".to_string(),
                9 => "CLOSED".to_string(),
                _ => "DISABLED".to_string(),
            }));
        }
        if let Some(v) = &data.locked_until {
            a.locked_until = Set(crate::state::ts_to_naive(v));
        }
    }
}

#[async_trait::async_trait]
impl proto::gen::services::UserServiceHandlers for UserService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListUserResponse, StatusError> {
        let tid = tenant_of(&ctx);
        let (rows, total) = self.repo(&ctx).paged_list(tid, &req).await?;
        Ok(ListUserResponse {
            items: rows
                .into_iter()
                .map(|u| user_to_proto(u, Vec::new()))
                .collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetUserRequest,
    ) -> Result<User, StatusError> {
        let tid = tenant_of(&ctx);
        let row = match req.query_by {
            Some(proto::proto::identity::service::v1::get_user_request::QueryBy::Id(id)) => {
                crate::data::sys_users::Entity::find_by_id(id)
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
            }
            Some(proto::proto::identity::service::v1::get_user_request::QueryBy::Username(
                name,
            )) => self.find_by_username(tid, &name).await?,
            None => None,
        }
        .ok_or_else(|| not_found("user"))?;
        let (_, codes) = load_user(&self.state, row.id).await?;
        Ok(user_to_proto(row, codes))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateUserRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
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
        let mut a = crate::data::sys_users::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            username: Set(username.clone()),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        Self::apply_user_fields(&mut a, &data);
        if a.status.clone().unwrap().is_none() {
            a.status = Set(Some("NORMAL".into()));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        self.upsert_credential(
            payload.tenant_id,
            inserted.id,
            &username,
            req.password.as_deref(),
        )
        .await?;
        self.sync_roles(payload.tenant_id, inserted.id, &data.role_ids)
            .await?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateUserRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::sys_users::Entity::find_by_id(req.id)
            .filter(crate::data::sys_users::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user"))?;
        let mut a: crate::data::sys_users::ActiveModel = row.into();
        let username_before = a.username.clone().unwrap().clone();
        if let Some(data) = &req.data {
            Self::apply_user_fields(&mut a, data);
            crate::stamp_update!(a, payload.user_id);
            a.update(&self.state.db).await.map_err(db_err)?;
            if !data.role_ids.is_empty() {
                self.sync_roles(payload.tenant_id, req.id, &data.role_ids)
                    .await?;
            }
        }
        if req.password.as_deref().is_some_and(|v| !v.is_empty()) {
            self.upsert_credential(
                payload.tenant_id,
                req.id,
                &username_before,
                req.password.as_deref(),
            )
            .await?;
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
            Some(proto::proto::identity::service::v1::delete_user_request::QueryBy::Id(id)) => id,
            Some(proto::proto::identity::service::v1::delete_user_request::QueryBy::Username(
                name,
            )) => self
                .find_by_username(tid, &name)
                .await?
                .map(|r| r.id)
                .ok_or_else(|| not_found("user"))?,
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        };
        crate::data::sys_user_credentials::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_credentials::Column::TenantId.eq(tid))
                    .add(crate::data::sys_user_credentials::Column::UserId.eq(id)),
            )
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_user_roles::Entity::delete_many()
            .filter(crate::data::sys_user_roles::Column::UserId.eq(id))
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        crate::data::sys_users::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
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
            Some(proto::proto::identity::service::v1::user_exists_request::QueryBy::Id(id)) => {
                crate::data::sys_users::Entity::find_by_id(id)
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
                    .is_some()
            }
            Some(proto::proto::identity::service::v1::user_exists_request::QueryBy::Username(
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
        let _ = operator_of(&ctx)?;
        // Plaintext over the wire here (the proto validates 8..=128);
        // complexity mirrors the policy checks.
        let min_len = crate::data::sys_configs::Entity::find()
            .filter(crate::data::sys_configs::Column::Key.eq("sys.password.minLen"))
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
        let hash = crate::crypto::hash_password(&req.new_password).map_err(internal_error)?;
        let cred = crate::data::sys_user_credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(crate::data::sys_user_credentials::Column::IdentityType.eq("USERNAME"))
                    .add(crate::data::sys_user_credentials::Column::UserId.eq(req.user_id)),
            )
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("user credential"))?;
        let mut a: crate::data::sys_user_credentials::ActiveModel = cred.into();
        a.credential = Set(hash);
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        self.state.tokens.revoke_user_token(req.user_id).await;
        Ok(Empty {})
    }
}

/// Loads the operator's user row with role codes resolved (the `/me`
/// shape).
pub(crate) async fn load_user(
    state: &AppState,
    uid: u32,
) -> Result<(crate::data::sys_users::Model, Vec<String>), crate::state::StatusError> {
    let user = crate::data::sys_users::Entity::find_by_id(uid)
        .one(&state.db)
        .await
        .map_err(|e| internal_error(format!("db: {e}")))?
        .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
    let role_ids: Vec<u32> = crate::data::sys_user_roles::Entity::find()
        .filter(crate::data::sys_user_roles::Column::UserId.eq(uid))
        .all(&state.db)
        .await
        .map_err(|e| internal_error(format!("db: {e}")))?
        .iter()
        .filter_map(|r| r.role_id)
        .collect();
    let codes = if role_ids.is_empty() {
        Vec::new()
    } else {
        crate::data::sys_roles::Entity::find()
            .filter(crate::data::sys_roles::Column::Id.is_in(role_ids))
            .all(&state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .into_iter()
            .map(|r| r.code)
            .collect()
    };
    Ok((user, codes))
}

/// User row → proto User (the identity fields; role codes ride the
/// `roles` list).
pub(crate) fn user_to_proto(user: crate::data::sys_users::Model, role_codes: Vec<String>) -> User {
    User {
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
        last_login_at: user.last_login_at.and_then(crate::state::naive_to_ts),
        last_login_ip: user.last_login_ip,
        status: user
            .status
            .as_deref()
            .map(|s| mapping::user_status_of(s).unwrap_or(0)),
        locked_until: user.locked_until.and_then(crate::state::naive_to_ts),
        created_by: user.created_by,
        updated_by: user.updated_by,
        deleted_by: user.deleted_by,
        created_at: user.created_at.and_then(crate::state::naive_to_ts),
        updated_at: user.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: user.deleted_at.and_then(crate::state::naive_to_ts),
        tenant_name: None,
        org_unit_id: None,
        org_unit_ids: Vec::new(),
        org_unit_name: None,
        org_unit_names: Vec::new(),
        position_id: None,
        position_ids: Vec::new(),
        position_name: None,
        position_names: Vec::new(),
        role_id: None,
        role_ids: Vec::new(),
        roles: role_codes,
        role_names: Vec::new(),
        remark: user.remark,
    }
}
