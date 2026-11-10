//! Token issuance and its bookkeeping: the access+refresh pair with
//! the Redis whitelist rows, session meta, and the refresh cookies.

use crate::admin::token::{SessionMeta, UserTokenPayload};
use rushwind_authn::Authenticator as _;
use rushwind_http_binding::envelope::StatusError;

use crate::admin::state::internal_error;

use super::{AuthenticationService, Ctx};

impl AuthenticationService {
    /// Mint the access+refresh pair and register the Redis rows.
    pub(super) async fn issue_token_pair(
        &self,
        payload: &UserTokenPayload,
    ) -> Result<(String, String), StatusError> {
        let now = chrono::Utc::now().timestamp();
        let access_exp = now + self.state.tokens.access_expires_secs;
        let refresh_exp = now + self.state.tokens.refresh_expires_secs;

        let access = self
            .state
            .jwt
            .create_identity(&rushwind_authn::AuthClaims(
                payload.to_access_claims(access_exp),
            ))
            .map_err(|_| internal_error("create access token failed"))?;
        let refresh = self
            .state
            .jwt
            .create_identity(&rushwind_authn::AuthClaims(
                payload.to_refresh_claims(refresh_exp),
            ))
            .map_err(|_| internal_error("create refresh token failed"))?;
        self.state
            .tokens
            .add_token_pair(payload.user_id, &payload.jti, &access, &refresh)
            .await
            .map_err(internal_error)?;
        Ok((access, refresh))
    }

    pub(super) fn session_meta(&self, payload: &UserTokenPayload) -> SessionMeta {
        SessionMeta {
            username: payload.username.clone(),
            tenant_id: payload.tenant_id,
            ip: String::new(),
            user_agent: String::new(),
            device: payload.device_id.clone(),
            login_at: chrono::Local::now().naive_local().to_string(),
        }
    }

    pub(super) async fn set_cookies(&self, ctx: &Ctx, refresh_token: &str, secure: bool) {
        let (rt, exp) = self
            .state
            .tokens
            .refresh_cookie_values(refresh_token, secure);
        ctx.add_reply_header("Set-Cookie", rt);
        ctx.add_reply_header("Set-Cookie", exp);
    }

    pub(super) fn clear_cookies(&self, ctx: &Ctx, secure: bool) {
        let (rt, exp) = crate::admin::token::TokenStore::clear_cookie_values(secure);
        ctx.add_reply_header("Set-Cookie", rt);
        ctx.add_reply_header("Set-Cookie", exp);
    }
}
