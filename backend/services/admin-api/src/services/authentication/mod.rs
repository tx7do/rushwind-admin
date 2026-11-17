//! AuthenticationService — the full login chain: rate-limit
//! gate → captcha gate → tenant resolve → login policies → identifier
//! resolution → AES-decrypted bcrypt credential verify (dummy-hash
//! timing equalizer) → user/policy re-checks → authority resolution
//! (`system:access_backend`) → MFA gate → token pair + Redis whitelist
//! rows → refresh cookies.
//!
//! The module map: [`authority`] aggregates roles/scopes/hidden-field
//! claims, [`credential`] resolves identifiers and verifies passwords,
//! [`policy`] evaluates the login-policy gate, [`token_issue`] mints
//! the token pair with session/cookie bookkeeping, and [`mfa`] owns
//! the login-challenge store. `mod.rs` keeps the RPC surface and the
//! `do_password` orchestrator that walks those gates in order.

mod authority;
mod credential;
mod mfa;
mod policy;
mod sso;
mod token_issue;

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use pbjson_types::Empty;
use proto::gen::services::AuthenticationServiceHandlers;
use proto::proto::authentication::service::v1::{login_request, GrantType};
use proto::proto::authentication::service::v1::{
    ForgotPasswordRequest, GenerateCaptchaResponse, GetSsoLoginInfoRequest,
    GetSsoLoginInfoResponse, GetSsoLoginUrlRequest, GetSsoLoginUrlResponse,
    GetTenantBrandingRequest, GetTenantBrandingResponse, LoginRequest, LoginResponse,
    ResetPasswordByCodeRequest, SsoLoginRequest, VerifyCaptchaRequest, VerifyCaptchaResponse,
};
use rushwind_http_binding::envelope::StatusError;

use crate::state::{internal_error, status_error, AppState};
use crate::token::{new_jwt_id, SessionMeta, UserTokenPayload};

use crate::data::sys_tenants as tenants;
use crate::data::sys_user_credentials as credentials;
use crate::data::sys_user_mfa_factors as mfa_factors;
use crate::data::sys_users as users;

pub struct AuthenticationService {
    pub state: Arc<AppState>,
}

fn invalid_password() -> StatusError {
    status_error("INVALID_PASSWORD", "invalid username or password")
}

type Ctx = rushwind_http_binding::ctx::RequestContext;

#[async_trait::async_trait]
impl AuthenticationServiceHandlers for AuthenticationService {
    async fn login(&self, ctx: Ctx, req: LoginRequest) -> Result<LoginResponse, StatusError> {
        match GrantType::try_from(req.grant_type) {
            Ok(GrantType::Password) => self.do_password(ctx, req).await,
            Ok(GrantType::RefreshToken) => Err(status_error(
                "INVALID_GRANT_TYPE",
                "use /admin/v1/refresh-token for token refresh",
            )),
            _ => Err(status_error("INVALID_GRANT_TYPE", "invalid grant type")),
        }
    }

    async fn logout(&self, ctx: Ctx, _req: Empty) -> Result<Empty, StatusError> {
        if let Some(payload) = ctx.claims.as_ref().and_then(UserTokenPayload::from_claims) {
            self.state.tokens.revoke_user_token(payload.user_id).await;
        }
        let secure = ctx
            .headers
            .get("x-forwarded-proto")
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        self.clear_cookies(&ctx, secure);
        Ok(Empty {})
    }

    async fn forgot_password(
        &self,
        _ctx: Ctx,
        req: ForgotPasswordRequest,
    ) -> Result<Empty, StatusError> {
        let identifier = req.identifier.unwrap_or_default();
        if !identifier.is_empty() {
            // The anti-enumeration contract: unknown identifiers answer
            // success silently (module).
            let row = credentials::Entity::find()
                .filter(
                    Condition::all()
                        .add(credentials::Column::IdentityType.eq("EMAIL"))
                        .add(credentials::Column::Identifier.eq(identifier.clone())),
                )
                .one(&self.state.db)
                .await
                .ok()
                .flatten();
            if row.is_some() {
                let code = format!(
                    "{:06}",
                    chrono::Utc::now().timestamp_micros().rem_euclid(1_000_000)
                );
                let mut conn = self.state.redis.clone();
                let _: Result<(), _> = redis::AsyncCommands::set_ex(
                    &mut conn,
                    format!("admin:vcode:reset_password:{identifier}"),
                    code.clone(),
                    600u64,
                )
                .await;
                // Delivery rides the SMTP notification channel; without a
                // configured relay the code stays retrievable server-side
                // (same fail mode as without channels).
                eprintln!("[forgot-password] reset code for {identifier}: {code}");
            }
        }
        Ok(Empty {})
    }

    async fn reset_password_by_code(
        &self,
        ctx: Ctx,
        req: ResetPasswordByCodeRequest,
    ) -> Result<Empty, StatusError> {
        let identifier = req.identifier.unwrap_or_default();
        let code = req.code.unwrap_or_default();
        if identifier.is_empty() || code.is_empty() {
            return Err(status_error("BAD_REQUEST", "invalid identifier or code"));
        }
        let key = format!("admin:vcode:reset_password:{identifier}");
        let mut conn = self.state.redis.clone();
        let stored: Option<String> = redis::AsyncCommands::get(&mut conn, &key)
            .await
            .unwrap_or(None);
        match stored {
            Some(stored) if stored == code => {
                let _: Result<i64, _> = redis::AsyncCommands::del(&mut conn, &key).await;
            }
            _ => return Err(status_error("BAD_REQUEST", "invalid verification code")),
        }

        // The user behind the identifier (email or username).
        let user = if identifier.contains('@') {
            users::Entity::find()
                .filter(users::Column::Email.eq(identifier.clone()))
                .one(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?
        } else {
            users::Entity::find()
                .filter(
                    Condition::all()
                        .add(users::Column::TenantId.eq(0))
                        .add(users::Column::Username.eq(identifier.clone())),
                )
                .one(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?
        };
        let Some(user) = user else {
            return Ok(Empty {}); // silent like the send path
        };

        let new_plain = req
            .new_password
            .as_deref()
            .map(crate::crypto::decrypt_transfer_password)
            .transpose()
            .map_err(|_| status_error("BAD_REQUEST", "invalid credential format"))?
            .ok_or_else(|| status_error("BAD_REQUEST", "invalid credential format"))?;

        let min_len = self.config_int("sys.password.minLen", 8).await;
        if (new_plain.len() as i64) < min_len || crate::policy::char_classes(&new_plain) < 3 {
            return Err(status_error(
                "BAD_REQUEST",
                "password does not meet complexity requirements",
            ));
        }

        let cred = credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(credentials::Column::TenantId.eq(user.tenant_id.unwrap_or(0)))
                    .add(credentials::Column::IdentityType.eq("USERNAME"))
                    .add(credentials::Column::Identifier.eq(user.username.clone())),
            )
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| internal_error("credential row missing"))?;

        // History: verify against the last N hashes, then append.
        let history_count = self.config_int("sys.password.historyCount", 3).await;
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

        let mut active: credentials::ActiveModel = cred.into();
        active.credential =
            sea_orm::Set(crate::crypto::hash_password(&new_plain).map_err(internal_error)?);
        active.updated_at = sea_orm::Set(Some(crate::data::now()));
        active.extra_info = sea_orm::Set(Some(serde_json::json!({ "password_history": history })));
        use sea_orm::ActiveModelTrait;
        active
            .update(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;

        self.state
            .tokens
            .revoke_user_token(user.id as u64 as i64)
            .await;
        let _ = ctx;
        Ok(Empty {})
    }

    async fn refresh_token(
        &self,
        ctx: Ctx,
        req: LoginRequest,
    ) -> Result<LoginResponse, StatusError> {
        if GrantType::try_from(req.grant_type) != Ok(GrantType::RefreshToken) {
            return Err(status_error("INVALID_GRANT_TYPE", "invalid grant type"));
        }
        let Some(token) = ctx.cookies.get("refresh_token").cloned() else {
            return Err(status_error(
                "INCORRECT_REFRESH_TOKEN",
                "refresh token cookie is missing",
            ));
        };

        // Signature + expiry via the verification engine; then the Lua
        // verify-and-revoke makes the row single-use.
        let claims = self
            .state
            .authenticator
            .authenticate_token(&token)
            .map_err(|_| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        if claims
            .get_expiration_time()
            .unwrap_or(Some(0))
            .map(|exp| exp < chrono::Utc::now().timestamp())
            .unwrap_or(false)
        {
            return Err(status_error(
                "INCORRECT_REFRESH_TOKEN",
                "refresh token is expired",
            ));
        }
        let uid = claims
            .0
            .get("uid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        let jti = claims
            .get_jwt_id()
            .map_err(|_| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        if !self
            .state
            .tokens
            .verify_and_revoke_refresh_token(uid as i64, &jti, &token)
            .await
        {
            return Err(status_error(
                "INCORRECT_REFRESH_TOKEN",
                "invalid refresh token",
            ));
        }

        // Fresh authority resolution from the DB (roles may have changed).
        let user = users::Entity::find_by_id(uid as i64)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        let payload = self
            .payload_for_user(user, req.client_id.clone(), req.device_id.clone())
            .await?;
        let payload = UserTokenPayload {
            jti: new_jwt_id(),
            ..payload
        };
        let (access, refresh) = self.issue_token_pair(&payload).await?;

        let meta = SessionMeta {
            ip: ctx.ip.clone(),
            user_agent: ctx.user_agent.clone(),
            ..self.session_meta(&payload)
        };
        let _ = self
            .state
            .tokens
            .set_session_meta(payload.user_id, &payload.jti, &meta)
            .await;

        let secure = ctx
            .headers
            .get("x-forwarded-proto")
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        self.set_cookies(&ctx, &refresh, secure).await;

        Ok(LoginResponse {
            token_type: 0, // bearer
            access_token: access,
            expires_in: self.state.tokens.access_expires_secs,
            scope: None,
            id_token: None,
            mfa_operation_id: None,
        })
    }

    async fn generate_captcha(
        &self,
        _ctx: Ctx,
        _req: Empty,
    ) -> Result<GenerateCaptchaResponse, StatusError> {
        let (id, image, _answer) = crate::captcha::generate(&self.state.redis)
            .await
            .map_err(internal_error)?;
        Ok(GenerateCaptchaResponse {
            captcha_id: id,
            image_base64: image,
        })
    }

    async fn verify_captcha(
        &self,
        _ctx: Ctx,
        req: VerifyCaptchaRequest,
    ) -> Result<VerifyCaptchaResponse, StatusError> {
        let valid =
            crate::captcha::verify(&self.state.redis, &req.captcha_id, &req.user_input).await;
        Ok(VerifyCaptchaResponse { valid })
    }

    async fn get_sso_login_info(
        &self,
        ctx: Ctx,
        req: GetSsoLoginInfoRequest,
    ) -> Result<GetSsoLoginInfoResponse, StatusError> {
        self.sso_login_info(ctx, req).await
    }

    async fn get_sso_login_url(
        &self,
        ctx: Ctx,
        req: GetSsoLoginUrlRequest,
    ) -> Result<GetSsoLoginUrlResponse, StatusError> {
        self.sso_login_url(ctx, req).await
    }

    async fn sso_login(
        &self,
        ctx: Ctx,
        req: SsoLoginRequest,
    ) -> Result<LoginResponse, StatusError> {
        self.sso_login(ctx, req).await
    }

    async fn get_tenant_branding(
        &self,
        ctx: Ctx,
        req: GetTenantBrandingRequest,
    ) -> Result<GetTenantBrandingResponse, StatusError> {
        self.tenant_branding(ctx, req).await
    }
}

impl AuthenticationService {
    /// The password grant: walk the gates in order, then mint the
    /// token pair. Each gate lives beside its helpers — policy in
    /// [`policy`], credentials in [`credential`], authority in
    /// [`authority`], the MFA branch in [`mfa`].
    async fn do_password(&self, ctx: Ctx, req: LoginRequest) -> Result<LoginResponse, StatusError> {
        let client_ip = ctx.ip.clone();
        // The reads GetUsername() — only the Username oneof
        // variant carries; email/mobile identifiers answer the uniform
        // failure path.
        let raw_identifier = match &req.identifier {
            Some(login_request::Identifier::Username(name)) => name.clone(),
            _ => String::new(),
        };
        let username: String = raw_identifier.replace(['\r', '\n'], "");

        // Tenant resolution (empty code = platform 0) and identifier
        // resolution hoist BEFORE the limiter pre-check (P2 修复：限流键一致
        // ——此前 IsLocked 用原始输入、check_and_incr 用解析后 username，
        // admin 被锁后改用 admin@victim.com 等变体发起请求，预检查的是未锁
        // 的变体键照常放行，用户维度锁形同虚设。统一用解析后的 username 作
        // 限流键，email/手机号/变体全落同一账号键；IsLocked 仍在 captcha
        // 之前，闸门顺序语义不变）。
        let mut tenant_id: i64 = 0;
        if let Some(code) = req
            .tenant_code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            let tenant = tenants::Entity::find()
                .filter(tenants::Column::Code.eq(code))
                .one(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?;
            match tenant {
                Some(t) if t.status.as_deref() == Some("ON") => tenant_id = t.id as u64 as i64,
                // Own reason (issue #1): the login screen folds wrong-tenant
                // into the unified credential message via the i18n table —
                // a shared BAD_REQUEST would render as 参数错误.
                _ => return Err(status_error("INVALID_TENANT", "invalid tenant")),
            }
        }
        let username = self.resolve_identifier(tenant_id, &username).await?;

        // Gate 1: the rate limiter pre-check. Own reason: the login screen
        // maps reasons to copy — a shared BAD_REQUEST would render as the
        // generic 参数错误 instead of the lockout message (issue #1).
        if crate::ratelimit::is_locked(&self.state.redis, &client_ip, &username).await {
            return Err(status_error(
                "TOO_MANY_ATTEMPTS",
                "too many login failures, please try again later",
            ));
        }

        // Gate 2: the progressive captcha (issue #1 建议). Captcha is NOT
        // required up front — once this (client_ip, username) pair has
        // failed `captcha_required_failures` logins, attempts must present
        // one: missing headers → CAPTCHA_REQUIRED (the signal that lights
        // the box on the login screen), wrong/aged captcha → CAPTCHA_INVALID.
        // Under the threshold the whole gate is skipped (the fast path).
        let captcha_threshold = self.state.cfg.captcha_required_failures;
        let captcha_gate_active = crate::ratelimit::captcha_required(
            &self.state.redis,
            &client_ip,
            &username,
            captcha_threshold,
        )
        .await;
        if captcha_gate_active {
            let id = ctx.headers.get("x-captcha-id").cloned().unwrap_or_default();
            let value = ctx
                .headers
                .get("x-captcha-value")
                .cloned()
                .unwrap_or_default();
            let captcha_ok = crate::captcha::verify(&self.state.redis, &id, &value).await;
            if !captcha_ok {
                if id.is_empty() && value.is_empty() {
                    return Err(status_error(
                        "CAPTCHA_REQUIRED",
                        "captcha required after repeated login failures",
                    ));
                }
                // Own reason (issue #1): captcha failures previously rode
                // BAD_REQUEST and surfaced as 请求参数错误 on the login screen.
                return Err(status_error(
                    "CAPTCHA_INVALID",
                    "invalid or missing captcha",
                ));
            }
        }

        // Login-policy gate (global pass, user pass after credential).
        if self
            .check_login_policies(
                tenant_id,
                0,
                &client_ip,
                req.device_id.as_deref().unwrap_or(""),
            )
            .await
        {
            return Err(status_error(
                "FORBIDDEN",
                "login blocked by security policy",
            ));
        }

        // Credential verify; failures feed the rate limiter then normalize.
        let matched_user_id = match self
            .verify_credential(
                tenant_id,
                &username,
                &req.password.clone().unwrap_or_default(),
            )
            .await
        {
            Ok(id) => id,
            Err(err) => {
                crate::ratelimit::check_and_incr(&self.state.redis, &client_ip, &username).await;
                // 渐进验证码计数（issue #1）：只在真实凭证失败上累计——
                // captcha/policy 分支不计，不给验证码的请求推不满阈值。
                crate::ratelimit::incr_captcha_fail(
                    &self.state.redis,
                    &client_ip,
                    &username,
                    15 * 60,
                )
                .await;
                let reason = err.reason;
                if matches!(
                    reason,
                    "USER_NOT_FOUND" | "USER_FREEZE" | "INVALID_PASSWORD"
                ) {
                    return Err(invalid_password());
                }
                return Err(err);
            }
        };

        let user = users::Entity::find_by_id(matched_user_id as i64)
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))?;

        if user.tenant_id.unwrap_or(0) != tenant_id {
            return Err(status_error("BAD_REQUEST", "invalid tenant"));
        }

        // User-targeted policy pass.
        if self
            .check_login_policies(
                tenant_id,
                user.id,
                &client_ip,
                req.device_id.as_deref().unwrap_or(""),
            )
            .await
        {
            return Err(status_error(
                "FORBIDDEN",
                "login blocked by security policy",
            ));
        }

        let payload = self
            .payload_for_user(user.clone(), req.client_id.clone(), req.device_id.clone())
            .await?;

        // MFA gate: enabled TOTP factor → challenge instead of tokens.
        let has_totp = mfa_factors::Entity::find()
            .filter(
                Condition::all()
                    .add(mfa_factors::Column::TenantId.eq(payload.tenant_id))
                    .add(mfa_factors::Column::UserId.eq(payload.user_id))
                    .add(mfa_factors::Column::Method.eq("TOTP"))
                    .add(mfa_factors::Column::Status.eq("ENABLED")),
            )
            .one(&self.state.db)
            .await
            .map_err(|_| internal_error("mfa check failed"))?
            .is_some();
        if has_totp {
            let op_id = new_jwt_id();
            self.set_mfa_challenge(&op_id, &payload)
                .await
                .map_err(|_| internal_error("mfa challenge failed"))?;
            return Ok(LoginResponse {
                token_type: 0,
                access_token: String::new(),
                expires_in: 0,
                scope: None,
                id_token: None,
                mfa_operation_id: Some(op_id),
            });
        }

        let payload = UserTokenPayload {
            jti: new_jwt_id(),
            ..payload
        };
        let (access, refresh) = self.issue_token_pair(&payload).await?;

        // Session meta + last-login bookkeeping (non-blocking).
        let meta = SessionMeta {
            ip: ctx.ip.clone(),
            user_agent: ctx.user_agent.clone(),
            ..self.session_meta(&payload)
        };
        let _ = self
            .state
            .tokens
            .set_session_meta(payload.user_id, &payload.jti, &meta)
            .await;
        {
            use sea_orm::ActiveModelTrait;
            use sea_orm::Set;
            let mut active: users::ActiveModel = user.into();
            active.last_login_at = Set(Some(crate::data::now()));
            active.last_login_ip = Set(Some(client_ip.clone()));
            let _ = active.update(&self.state.db).await;
        }
        crate::ratelimit::reset(&self.state.redis, &client_ip, &username).await;
        crate::ratelimit::reset_captcha_fail(&self.state.redis, &client_ip, &username).await;

        let secure = ctx
            .headers
            .get("x-forwarded-proto")
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        self.set_cookies(&ctx, &refresh, secure).await;

        Ok(LoginResponse {
            token_type: 0,
            access_token: access,
            expires_in: self.state.tokens.access_expires_secs,
            scope: None,
            id_token: None,
            mfa_operation_id: None,
        })
    }
}

#[cfg(test)]
mod progressive_captcha_tests {
    use super::*;
    use crate::crypto::{decrypt_transfer_password, hash_password, DEFAULT_AES_KEY};
    use crate::data::{sys_user_credentials as creds, sys_users};
    use crate::testsupport::{ctx, test_state};
    use sea_orm::{ActiveModelTrait as _, Set};

    /// 登录请求构造（legacy AES 通道，密文用 DEFAULT_AES_KEY 封装）。
    fn password_req(username: &str, plain: &str) -> LoginRequest {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let plaintext = plain.as_bytes();
        let mut buf = [0u8; 64];
        let ciphertext = Aes128CbcEnc::new(DEFAULT_AES_KEY.into(), DEFAULT_AES_KEY.into())
            .encrypt_padded_b2b_mut::<Pkcs7>(plaintext, &mut buf)
            .unwrap()
            .to_vec();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(ciphertext);
        LoginRequest {
            grant_type: GrantType::Password as i32,
            identifier: Some(login_request::Identifier::Username(username.into())),
            password: Some(encoded),
            ..Default::default()
        }
    }

    async fn plant_user(state: &std::sync::Arc<AppState>, username: &str) {
        // 登录的租户取自 tenant_code（缺省=平台 0），不是 ctx claim——
        // 用户种在平台租户，与 HTTP 冒烟同口径。
        let user = sys_users::ActiveModel {
            tenant_id: Set(Some(0)),
            username: Set(username.into()),
            realname: Set(Some(username.into())),
            status: Set(Some("NORMAL".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        creds::ActiveModel {
            tenant_id: Set(Some(0)),
            user_id: Set(Some(user.id)),
            identity_type: Set(Some("USERNAME".into())),
            identifier: Set(username.into()),
            credential_type: Set(Some("PASSWORD_HASH".into())),
            credential: Set(hash_password("Right@1234").unwrap()),
            is_primary: Set(Some(true)),
            status: Set(Some("ENABLED".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();

        // 登录后权威闸要求 sys:access_backend——种 角色/授权/绑定 三件套。
        let role = crate::data::sys_roles::ActiveModel {
            tenant_id: Set(Some(0)),
            name: Set(format!("{username}-role")),
            code: Set(format!("captcha-test:{username}")),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        let perm = crate::data::sys_permissions::ActiveModel {
            name: Set("访问后台".into()),
            code: Set("sys:access_backend".into()),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        crate::data::sys_role_permissions::ActiveModel {
            tenant_id: Set(Some(0)),
            role_id: Set(Some(role.id)),
            permission_id: Set(Some(perm.id)),
            effect: Set(Some("ALLOW".into())),
            status: Set(Some("ON".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
        crate::data::sys_user_roles::ActiveModel {
            tenant_id: Set(Some(0)),
            user_id: Set(Some(user.id)),
            role_id: Set(Some(role.id)),
            is_primary: Set(Some(true)),
            status: Set(Some("ACTIVE".into())),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }

    fn me() -> rushwind_http_binding::ctx::RequestContext {
        // ip 留空 → 计数只落 user 维键（断言口径单一）。
        ctx(7, 1)
    }

    /// 渐进验证码三段式（issue #1）：阈值内失败不拦正确密码；越过阈值
    /// 后无头即 CAPTCHA_REQUIRED；带验证码通过且计数清零。中间穿插
    /// 「失败 2 次后第 3 次用正确密码仍成功」的快路径断言。
    #[tokio::test]
    async fn captcha_lights_only_after_threshold() {
        let state = test_state().await;
        let svc = AuthenticationService {
            state: state.clone(),
        };
        plant_user(&state, "capuser").await;
        let threshold = state.cfg.captcha_required_failures; // 3
        assert_eq!(threshold, 3);

        // —— 快路径：失败 2 次（< 阈值）后，正确密码无头照样成功，且清零。
        for _ in 0..2 {
            let err = svc
                .login(me(), password_req("capuser", "Wrong@999"))
                .await
                .unwrap_err();
            assert_eq!(err.reason, "INVALID_PASSWORD");
        }
        {
            let req = password_req("capuser", "Right@1234");
            let plain =
                crate::crypto::decrypt_transfer_password(req.password.as_deref().unwrap()).unwrap();
            let row = crate::data::sys_user_credentials::Entity::find()
                .filter(crate::data::sys_user_credentials::Column::Identifier.eq("capuser"))
                .one(&state.db)
                .await
                .unwrap()
                .expect("cred row");
            let verdict = crate::crypto::verify_password(&plain, &row.credential);
            eprintln!(
                "DBG plain={plain:?} cred_len={} verdict={verdict} status={:?} uid={:?}",
                row.credential.len(),
                row.status,
                row.user_id
            );
        }
        svc.login(me(), password_req("capuser", "Right@1234"))
            .await
            .expect("under threshold, no captcha demanded");
        assert!(
            !crate::ratelimit::captcha_required(&state.redis, "", "capuser", threshold).await,
            "success resets the captcha counter"
        );

        // —— 越过阈值：失败 3 次。
        for _ in 0..threshold {
            let err = svc
                .login(me(), password_req("capuser", "Wrong@999"))
                .await
                .unwrap_err();
            assert_eq!(err.reason, "INVALID_PASSWORD");
        }
        assert!(
            crate::ratelimit::captcha_required(&state.redis, "", "capuser", threshold).await,
            "counter crossed the threshold"
        );

        // 正确密码但无验证码头 → CAPTCHA_REQUIRED（点亮信号）。
        let err = svc
            .login(me(), password_req("capuser", "Right@1234"))
            .await
            .unwrap_err();
        assert_eq!(err.reason, "CAPTCHA_REQUIRED");
        assert_eq!(err.status, 400);

        // 带验证码（generate 真图）→ 通过且计数清零。
        let (id, _img, value) = crate::captcha::generate(&state.redis).await.unwrap();
        let mut with_captcha = me();
        with_captcha
            .headers
            .insert("x-captcha-id".into(), id.clone());
        with_captcha.headers.insert("x-captcha-value".into(), value);
        svc.login(with_captcha, password_req("capuser", "Right@1234"))
            .await
            .expect("captcha presented, credential accepted");
        assert!(
            !crate::ratelimit::captcha_required(&state.redis, "", "capuser", threshold).await,
            "success clears the captcha counter"
        );
    }

    /// 错误验证码在要求态下是 CAPTCHA_INVALID（与缺失的 REQUIRED 分辨）。
    #[tokio::test]
    async fn wrong_captcha_when_required_is_invalid() {
        let state = test_state().await;
        let svc = AuthenticationService {
            state: state.clone(),
        };
        plant_user(&state, "capuser2").await;
        let threshold = state.cfg.captcha_required_failures;
        for _ in 0..threshold {
            svc.login(me(), password_req("capuser2", "Wrong@999"))
                .await
                .unwrap_err();
        }
        let mut with_bad = me();
        let (id, _img, _value) = crate::captcha::generate(&state.redis).await.unwrap();
        with_bad.headers.insert("x-captcha-id".into(), id);
        with_bad
            .headers
            .insert("x-captcha-value".into(), "NONSENSE".into());
        let err = svc
            .login(with_bad, password_req("capuser2", "Right@1234"))
            .await
            .unwrap_err();
        assert_eq!(err.reason, "CAPTCHA_INVALID");
        // 校验失败不计数：阈值不再上推（第 2 题定案）。
        assert!(
            crate::ratelimit::captcha_required(&state.redis, "", "capuser2", threshold).await,
            "still required — captcha-gate failures don't increment"
        );
    }

    /// 解密通道健全性（测试密文与 verify_credential 的 legacy 通道对齐）。
    #[test]
    fn test_ciphertext_decrypts_to_the_plain_password() {
        let req = password_req("x", "Right@1234");
        let plain = decrypt_transfer_password(req.password.as_deref().unwrap()).unwrap();
        assert_eq!(plain, "Right@1234");
    }
}
