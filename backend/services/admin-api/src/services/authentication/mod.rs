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

        self.state.tokens.revoke_user_token(user.id).await;
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
            .map(|v| v as u32)
            .ok_or_else(|| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        let jti = claims
            .get_jwt_id()
            .map_err(|_| status_error("INCORRECT_REFRESH_TOKEN", "invalid refresh token"))?;
        if !self
            .state
            .tokens
            .verify_and_revoke_refresh_token(uid, &jti, &token)
            .await
        {
            return Err(status_error(
                "INCORRECT_REFRESH_TOKEN",
                "invalid refresh token",
            ));
        }

        // Fresh authority resolution from the DB (roles may have changed).
        let user = users::Entity::find_by_id(uid)
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

        // Gate 1: the rate limiter pre-check.
        if crate::ratelimit::is_locked(&self.state.redis, &client_ip, &username).await {
            return Err(status_error(
                "BAD_REQUEST",
                "too many login failures, please try again later",
            ));
        }

        // Gate 2: the mandatory captcha (headers X-Captcha-Id/X-Captcha-Value).
        let captcha_ok = {
            let id = ctx.headers.get("x-captcha-id").cloned().unwrap_or_default();
            let value = ctx
                .headers
                .get("x-captcha-value")
                .cloned()
                .unwrap_or_default();
            crate::captcha::verify(&self.state.redis, &id, &value).await
        };
        if !captcha_ok {
            return Err(status_error("BAD_REQUEST", "invalid or missing captcha"));
        }

        // Tenant resolution: empty code = platform (0).
        let mut tenant_id: u32 = 0;
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
                Some(t) if t.status.as_deref() == Some("ON") => tenant_id = t.id,
                _ => return Err(status_error("BAD_REQUEST", "invalid tenant")),
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

        // Identifier resolution (email / mobile / username).
        let username = self.resolve_identifier(tenant_id, &username).await?;

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

        let user = users::Entity::find_by_id(matched_user_id)
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
