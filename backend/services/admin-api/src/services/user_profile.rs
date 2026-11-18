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
            // update_mask 白名单门（字段级更新语义）：无掩码 = 存量全量
            // 提交语义；有掩码只有选中字段可动。/me 面只暴露个人资料
            // 字段——username/avatar/status 等敏感列不在本面可写。
            let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
            crate::set_masked!(opt mask, active, nickname, data.nickname, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, realname, data.realname, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, email, data.email, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, mobile, data.mobile, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, telephone, data.telephone, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, address, data.address, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(opt mask, active, region, data.region, |v: &String| Some(
                v.clone()
            ));
            crate::set_masked!(
                opt mask,
                active,
                description,
                data.description,
                |v: &String| Some(v.clone())
            );
            crate::set_masked!(opt mask, active, gender, data.gender, |v: &i32| Some(
                match v {
                    1 => "MALE".to_string(),
                    2 => "FEMALE".to_string(),
                    _ => "SECRET".to_string(),
                }
            ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::sys_users as users;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    /// /me 面的固定主角：uid=5（ctx(1, 5) 的视角）。
    async fn plant_me(db: &sea_orm::DatabaseConnection) -> users::Model {
        users::ActiveModel {
            id: Set(5),
            tenant_id: Set(Some(1)),
            username: Set("me".into()),
            nickname: Set(Some("旧昵称".into())),
            realname: Set(Some("旧名".into())),
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

    fn update_req(data: User, paths: &[&str]) -> UpdateUserRequest {
        UpdateUserRequest {
            id: 5,
            data: Some(data),
            password: None,
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门（/me 面）：掩码只选 email 时，载荷里多带的
    /// mobile 与 realname 不许落列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = UserProfileService {
            state: state.clone(),
        };
        let me = ctx(1, 5);
        let db = &state.db;
        plant_me(db).await;

        svc.update_user(
            me,
            update_req(
                User {
                    email: Some("me@corp.dev".into()),
                    mobile: Some("19999999999".into()),
                    realname: Some("越权名".into()),
                    ..Default::default()
                },
                &["email"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, 5).await;
        assert_eq!(
            fresh.email.as_deref(),
            Some("me@corp.dev"),
            "masked field lands"
        );
        assert_eq!(
            fresh.mobile.as_deref(),
            Some("13800000000"),
            "unmasked mobile is not writable through the gate"
        );
        assert_eq!(
            fresh.realname.as_deref(),
            Some("旧名"),
            "unmasked realname is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（可空列清值通道）；无掩码 None
    /// 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = UserProfileService {
            state: state.clone(),
        };
        let db = &state.db;
        plant_me(db).await;

        // 无掩码：None 不动列。
        svc.update_user(
            ctx(1, 5),
            UpdateUserRequest {
                id: 5,
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
        let fresh = row_by_id(db, 5).await;
        assert_eq!(fresh.nickname.as_deref(), Some("新昵称"));
        assert_eq!(
            fresh.mobile.as_deref(),
            Some("13800000000"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中 mobile、载荷 None：可空 mobile 清列，未选中的 nickname
        // 保持刚写入的值。
        svc.update_user(ctx(1, 5), update_req(User::default(), &["mobile"]))
            .await
            .unwrap();
        let fresh = row_by_id(db, 5).await;
        assert!(
            fresh.mobile.is_none(),
            "masked None explicitly clears the column"
        );
        assert_eq!(
            fresh.nickname.as_deref(),
            Some("新昵称"),
            "unmasked nickname keeps its value"
        );
    }
}
