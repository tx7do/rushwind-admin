//! UserProfileService — the `/me` surface (module):
//! own-user fetch/update, password change (old verify + history +
//! complexity + full token revocation), avatar bind (data-URL storage in
//! the same avatar column), contact bind/verify stubs.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use pbjson_types::Empty;
use proto::gen::services::UserProfileServiceHandlers;
use proto::proto::identity::service::v1::{
    BindContactRequest, ChangePasswordRequest, UpdateUserRequest, UploadAvatarRequest,
    UploadAvatarResponse, User, VerifyContactRequest,
};

use crate::services::user::{load_user, user_to_proto};
use crate::state::{internal_error, status_error, AppState};

pub struct UserProfileService {
    pub state: Arc<AppState>,
}

impl UserProfileService {
    /// ChangeCredential: verify old, enforce complexity + history, append
    /// old hash, revoke every session of the user.
    async fn change_credential(
        &self,
        uid: i64,
        old_encrypted: &str,
        new_encrypted: &str,
    ) -> Result<(), crate::state::StatusError> {
        let decode = |v: &str| {
            crate::crypto::decrypt_transfer_password(v)
                .map_err(|_| status_error("BAD_REQUEST", "invalid credential format"))
        };
        let old_plain = decode(old_encrypted)?;
        let new_plain = decode(new_encrypted)?;

        let payload = crate::data::sys_user_roles::Entity::find()
            .filter(crate::data::sys_user_roles::Column::UserId.eq(uid))
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        let tenant_id = payload.and_then(|r| r.tenant_id).unwrap_or(0);
        let user = crate::data::sys_users::Entity::find_by_id(uid)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;

        let cred = crate::data::sys_user_credentials::Entity::find()
            .filter(
                sea_orm::sea_query::Condition::all()
                    .add(crate::data::sys_user_credentials::Column::TenantId.eq(tenant_id))
                    .add(crate::data::sys_user_credentials::Column::IdentityType.eq("USERNAME"))
                    .add(crate::data::sys_user_credentials::Column::Identifier.eq(user.username)),
            )
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("BAD_REQUEST", "invalid old password"))?;
        if !crate::crypto::verify_password(&old_plain, &cred.credential) {
            return Err(status_error("BAD_REQUEST", "invalid old password"));
        }

        let min_len = config_int(&self.state, "sys.password.minLen", 8).await;
        if (new_plain.len() as i64) < min_len || crate::policy::char_classes(&new_plain) < 3 {
            return Err(status_error(
                "BAD_REQUEST",
                "password does not meet complexity requirements",
            ));
        }
        let history_count = config_int(&self.state, "sys.password.historyCount", 3).await;
        let mut history: Vec<String> = cred
            .extra_info
            .as_ref()
            .and_then(|v| v.get("password_history"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        if history
            .iter()
            .any(|h| crate::crypto::verify_password(&new_plain, h))
        {
            return Err(status_error("BAD_REQUEST", "password was used recently"));
        }
        history.push(cred.credential.clone());
        let keep = history.len().saturating_sub(history_count.max(0) as usize);
        history.drain(..keep);

        let mut active: crate::data::sys_user_credentials::ActiveModel = cred.into();
        active.credential = Set(crate::crypto::hash_password(&new_plain).map_err(internal_error)?);
        active.updated_at = Set(Some(crate::data::now()));
        active.extra_info = Set(Some(serde_json::json!({ "password_history": history })));
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;

        self.state.tokens.revoke_user_token(uid).await;
        Ok(())
    }
}

async fn config_int(state: &AppState, key: &str, default: i64) -> i64 {
    crate::data::sys_configs::Entity::find()
        .filter(crate::data::sys_configs::Column::Key.eq(key))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.value)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[async_trait::async_trait]
impl UserProfileServiceHandlers for UserProfileService {
    async fn get_user(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<User, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let (user, codes) = load_user(&self.state, payload.user_id).await?;
        Ok(user_to_proto(user, codes))
    }

    async fn update_user(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateUserRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let row = crate::data::sys_users::Entity::find_by_id(payload.user_id)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
        let mut active: crate::data::sys_users::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.nickname {
                active.nickname = Set(Some(v.clone()));
            }
            if let Some(v) = &data.realname {
                active.realname = Set(Some(v.clone()));
            }
            if let Some(v) = &data.email {
                active.email = Set(Some(v.clone()));
            }
            if let Some(v) = &data.mobile {
                active.mobile = Set(Some(v.clone()));
            }
            if let Some(v) = &data.telephone {
                active.telephone = Set(Some(v.clone()));
            }
            if let Some(v) = &data.address {
                active.address = Set(Some(v.clone()));
            }
            if let Some(v) = &data.region {
                active.region = Set(Some(v.clone()));
            }
            if let Some(v) = &data.description {
                active.description = Set(Some(v.clone()));
            }
            if let Some(v) = data.gender {
                active.gender = Set(Some(match v {
                    1 => "MALE".to_string(),
                    2 => "FEMALE".to_string(),
                    _ => "SECRET".to_string(),
                }));
            }
        }
        crate::stamp_update!(active, payload.user_id);
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(Empty {})
    }

    async fn change_password(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ChangePasswordRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        self.change_credential(payload.user_id, &req.old_password, &req.new_password)
            .await?;
        Ok(Empty {})
    }

    async fn upload_avatar(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UploadAvatarRequest,
    ) -> Result<UploadAvatarResponse, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        // Data-URL storage in the avatar column: self-contained without an
        // object store. (The uploads to MinIO and stores the link.)
        let data_url = match &req.source {
            Some(
                proto::proto::identity::service::v1::upload_avatar_request::Source::ImageBase64(
                    b64,
                ),
            ) => {
                if b64.starts_with("data:") {
                    b64.clone()
                } else {
                    format!("data:image/png;base64,{b64}")
                }
            }
            Some(proto::proto::identity::service::v1::upload_avatar_request::Source::ImageUrl(
                url,
            )) => url.clone(),
            None => return Err(status_error("BAD_REQUEST", "missing avatar source")),
        };
        let row = crate::data::sys_users::Entity::find_by_id(payload.user_id)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
        let mut active: crate::data::sys_users::ActiveModel = row.into();
        active.avatar = Set(Some(data_url.clone()));
        active.updated_at = Set(Some(crate::data::now()));
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(UploadAvatarResponse { url: data_url })
    }

    async fn delete_avatar(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let row = crate::data::sys_users::Entity::find_by_id(payload.user_id)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
        let mut active: crate::data::sys_users::ActiveModel = row.into();
        active.avatar = Set(None);
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(Empty {})
    }

    async fn bind_contact(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: BindContactRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        let payload = crate::state::operator_of(&ctx)?;
        let row = crate::data::sys_users::Entity::find_by_id(payload.user_id)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;
        let mut active: crate::data::sys_users::ActiveModel = row.into();
        match req.contact {
            Some(proto::proto::identity::service::v1::bind_contact_request::Contact::Phone(p)) => {
                active.mobile = Set(Some(p.phone));
            }
            Some(proto::proto::identity::service::v1::bind_contact_request::Contact::Email(e)) => {
                active.email = Set(Some(e.email));
            }
            None => return Err(status_error("BAD_REQUEST", "missing contact")),
        }
        active.updated_at = Set(Some(crate::data::now()));
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        Ok(Empty {})
    }

    async fn verify_contact(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: VerifyContactRequest,
    ) -> Result<Empty, crate::state::StatusError> {
        // No SMS/email gateway is wired;
        // treats this as unverified-but-accepted.
        let _ = crate::state::operator_of(&ctx)?;
        Ok(Empty {})
    }
}
