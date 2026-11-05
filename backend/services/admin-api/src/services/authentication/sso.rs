//! OIDC SSO login — the environment-opt-in identity face that replaced
//! the retired oauth.proto upstream.
//!
//! Config rides `SSO_OIDC_*` environment variables (issuer / client id /
//! client secret / redirect URL, plus scopes, auto-create and the tenant
//! code); the capability opens when the four base values are all set.
//! Flow per the reference implementation: state is minted server-side and
//! parked in Redis for ten minutes, single-use (`Take` validates and
//! deletes — CSRF and callback-replay protection); the authorization URL
//! builds off the IdP's discovery document (cached in-process for an
//! hour); `sso_login` trades the callback code at the token endpoint,
//! reads the email claim off userinfo, locates — or optionally
//! auto-provisions — the local account, then rides the exact same
//! authority + token-pair machinery as the password login. V1 boundary:
//! no local TOTP MFA gate, no rate limiter, no login policies — the
//! password-less path has no brute-force surface and authentication has
//! already happened at the IdP.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use proto::proto::authentication::service::v1::{
    GetSsoLoginInfoRequest, GetSsoLoginInfoResponse, GetSsoLoginUrlRequest, GetSsoLoginUrlResponse,
    GetTenantBrandingRequest, GetTenantBrandingResponse, LoginResponse, SsoLoginRequest,
};

use crate::data::sys_tenants as tenants;
use crate::data::sys_users as users;
use crate::state::{internal_error, status_error, StatusError};

use super::{AuthenticationService, Ctx};

/// The env-parsed OIDC config; `enabled` opens when the four base
/// values are all present.
#[derive(Clone, Debug)]
struct SsoConfig {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_url: String,
    scopes: String,
    auto_create: bool,
    tenant_code: String,
}

impl SsoConfig {
    fn from_env() -> Self {
        fn env(key: &str) -> String {
            std::env::var(key).unwrap_or_default().trim().to_string()
        }
        Self {
            issuer: env("SSO_OIDC_ISSUER"),
            client_id: env("SSO_OIDC_CLIENT_ID"),
            client_secret: env("SSO_OIDC_CLIENT_SECRET"),
            redirect_url: env("SSO_OIDC_REDIRECT_URL"),
            scopes: {
                let v = env("SSO_OIDC_SCOPES");
                if v.is_empty() {
                    "openid email profile".to_string()
                } else {
                    v
                }
            },
            auto_create: truthy_env(&env("SSO_AUTO_CREATE")),
            tenant_code: env("SSO_TENANT_CODE"),
        }
    }

    fn enabled(&self) -> bool {
        !self.issuer.is_empty()
            && !self.client_id.is_empty()
            && !self.client_secret.is_empty()
            && !self.redirect_url.is_empty()
    }
}

/// The env truthiness set the reference accepts.
fn truthy_env(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The three discovery endpoints this flow consumes.
#[derive(Clone, Debug)]
struct SsoDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

const DISCOVERY_TTL: Duration = Duration::from_secs(3600);
const STATE_TTL_SECS: u64 = 600;

/// In-process discovery cache (issuer → doc + expiry). A singleton map
/// is enough: discovery fingerprints rarely change, and a stale entry
/// just re-fetches on the next login.
static DISCOVERY_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (SsoDiscovery, Instant)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// The outbound HTTP client (15s timeout, matching the reference).
static SSO_HTTP: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("sso http client")
});

async fn fetch_discovery(issuer: &str) -> Result<SsoDiscovery, StatusError> {
    {
        let cache = DISCOVERY_CACHE.lock().expect("discovery cache");
        if let Some((doc, exp)) = cache.get(issuer) {
            if Instant::now() < *exp {
                return Ok(doc.clone());
            }
        }
    }
    let well_known = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let resp = SSO_HTTP
        .get(&well_known)
        .send()
        .await
        .map_err(|_| status_error("INTERNAL_SERVER_ERROR", "fetch sso discovery failed"))?;
    if !resp.status().is_success() {
        return Err(status_error(
            "INTERNAL_SERVER_ERROR",
            "fetch sso discovery failed",
        ));
    }
    #[derive(serde::Deserialize)]
    struct RawDoc {
        authorization_endpoint: String,
        token_endpoint: String,
        userinfo_endpoint: String,
    }
    let raw: RawDoc = resp
        .json()
        .await
        .map_err(|_| status_error("INTERNAL_SERVER_ERROR", "fetch sso discovery failed"))?;
    if raw.authorization_endpoint.is_empty()
        || raw.token_endpoint.is_empty()
        || raw.userinfo_endpoint.is_empty()
    {
        return Err(status_error(
            "INTERNAL_SERVER_ERROR",
            "fetch sso discovery failed",
        ));
    }
    let doc = SsoDiscovery {
        authorization_endpoint: raw.authorization_endpoint,
        token_endpoint: raw.token_endpoint,
        userinfo_endpoint: raw.userinfo_endpoint,
    };
    DISCOVERY_CACHE.lock().expect("discovery cache").insert(
        issuer.to_string(),
        (doc.clone(), Instant::now() + DISCOVERY_TTL),
    );
    Ok(doc)
}

/// Park a login state in Redis, single-use, ten minutes.
async fn sso_state_put(
    redis: &redis::aio::ConnectionManager,
    state: &str,
) -> Result<(), StatusError> {
    let mut conn = redis.clone();
    let key = format!("rushwind:sso:state:{state}");
    redis::cmd("SET")
        .arg(&key)
        .arg(1)
        .arg("EX")
        .arg(STATE_TTL_SECS as i64)
        .arg("NX")
        .query_async::<()>(&mut conn)
        .await
        .map_err(|e| internal_error(e.to_string()))
}

/// Validate-and-delete the state (single use: a replayed callback fails here).
async fn sso_state_take(
    redis: &redis::aio::ConnectionManager,
    state: &str,
) -> Result<bool, StatusError> {
    let mut conn = redis.clone();
    let key = format!("rushwind:sso:state:{state}");
    redis::cmd("GETDEL")
        .arg(&key)
        .query_async::<Option<String>>(&mut conn)
        .await
        .map(|v| v.is_some())
        .map_err(|e| internal_error(e.to_string()))
}

impl AuthenticationService {
    /// SSO capability switch (drives the login page's button visibility).
    pub(super) async fn sso_login_info(
        &self,
        _ctx: Ctx,
        _req: GetSsoLoginInfoRequest,
    ) -> Result<GetSsoLoginInfoResponse, StatusError> {
        Ok(GetSsoLoginInfoResponse {
            enabled: SsoConfig::from_env().enabled(),
        })
    }

    /// Build the OIDC authorization URL; the minted state parks in Redis.
    pub(super) async fn sso_login_url(
        &self,
        _ctx: Ctx,
        _req: GetSsoLoginUrlRequest,
    ) -> Result<GetSsoLoginUrlResponse, StatusError> {
        let cfg = SsoConfig::from_env();
        if !cfg.enabled() {
            return Err(status_error("BAD_REQUEST", "sso is not configured"));
        }
        let discovery = fetch_discovery(&cfg.issuer).await?;
        let state = crate::token::new_jwt_id();
        sso_state_put(&self.state.redis, &state).await?;

        let auth_url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            discovery.authorization_endpoint,
            urlencoding(&cfg.client_id),
            urlencoding(&cfg.redirect_url),
            urlencoding(&cfg.scopes),
            urlencoding(&state),
        );
        Ok(GetSsoLoginUrlResponse {
            authorization_url: auth_url,
            state,
        })
    }

    /// The OIDC callback: state check → code exchange → userinfo email →
    /// local account (optionally auto-provisioned) → the standard token
    /// pair with refresh cookies, same authority machinery as the
    /// password login. No MFA/rate-limit/login-policy gates by design.
    pub(super) async fn sso_login(
        &self,
        ctx: Ctx,
        req: SsoLoginRequest,
    ) -> Result<LoginResponse, StatusError> {
        let cfg = SsoConfig::from_env();
        if !cfg.enabled() {
            return Err(status_error("BAD_REQUEST", "sso is not configured"));
        }
        if req.code.is_empty() || req.state.is_empty() {
            return Err(status_error("BAD_REQUEST", "code and state are required"));
        }

        // 1. state single-use validate-and-delete.
        if !sso_state_take(&self.state.redis, &req.state).await? {
            return Err(status_error("UNAUTHORIZED", "sso state invalid or expired"));
        }

        let discovery = fetch_discovery(&cfg.issuer).await?;

        // 2. exchange the code at the token endpoint (client_secret_post).
        let idp_token = exchange_code(&discovery.token_endpoint, &cfg, &req.code).await?;

        // 3. userinfo: the email claim anchors the local mapping.
        let claims = fetch_userinfo(&discovery.userinfo_endpoint, &idp_token).await?;
        let email = claims.get("email").map(|s| s.trim()).unwrap_or("");
        if email.is_empty() {
            return Err(status_error(
                "UNAUTHORIZED",
                "sso userinfo has no email claim",
            ));
        }

        // 4. locate or provision the local account.
        let mut tenant_id: u32 = 0;
        if !cfg.tenant_code.is_empty() {
            let tenant = tenants::Entity::find()
                .filter(tenants::Column::Code.eq(cfg.tenant_code.as_str()))
                .one(&self.state.db)
                .await
                .map_err(|e| internal_error(format!("db: {e}")))?;
            match tenant {
                Some(t) => tenant_id = t.id,
                None => {
                    return Err(status_error(
                        "INTERNAL_SERVER_ERROR",
                        "sso tenant misconfigured",
                    ));
                }
            }
        }

        let user = users::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(users::Column::TenantId.eq(tenant_id))
                    .add(users::Column::Email.eq(email)),
            )
            .one(&self.state.db)
            .await
            .map_err(|e| internal_error(format!("db: {e}")))?;
        let user = match user {
            Some(u) => u,
            None if cfg.auto_create => self.sso_auto_create_user(tenant_id, email).await?,
            None => {
                return Err(status_error(
                    "FORBIDDEN",
                    "no local account matches the sso identity; ask an administrator to create one",
                ));
            }
        };
        if user.status.as_deref() != Some("NORMAL") {
            return Err(status_error("FORBIDDEN", "account is frozen"));
        }

        // 5. authority + token pair — the password-login machinery.
        let payload = self
            .payload_for_user(user.clone(), Some("sso".into()), None)
            .await?;
        let (access, refresh) = self.issue_token_pair(&payload).await?;

        let meta = crate::token::SessionMeta {
            ip: String::new(),
            user_agent: String::new(),
            ..self.session_meta(&payload)
        };
        let _ = self
            .state
            .tokens
            .set_session_meta(payload.user_id, &payload.jti, &meta)
            .await;
        {
            use sea_orm::ActiveModelTrait;
            let mut active: users::ActiveModel = user.into();
            active.last_login_at = Set(Some(crate::data::now()));
            let _ = active.update(&self.state.db).await;
        }

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

    /// Auto-provision by email: username `sso_<local>_<8>`, no password
    /// credential (password login stays impossible until an admin sets
    /// one), status NORMAL.
    async fn sso_auto_create_user(
        &self,
        tenant_id: u32,
        email: &str,
    ) -> Result<users::Model, StatusError> {
        let local = email.split('@').next().unwrap_or(email);
        let suffix: String = crate::token::new_jwt_id().chars().take(8).collect();
        let username = format!("sso_{local}_{suffix}");
        users::ActiveModel {
            tenant_id: Set(Some(tenant_id)),
            username: Set(username),
            email: Set(Some(email.to_string())),
            status: Set(Some("NORMAL".into())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(|e| internal_error(format!("db: {e}")))
    }

    /// Tenant white-labeling (pre-login, auth-free). Only display fields
    /// leave — an unknown code answers `found: false` exactly like an
    /// unconfigured one, so the endpoint leaks no enumeration signal.
    pub(super) async fn tenant_branding(
        &self,
        _ctx: Ctx,
        req: GetTenantBrandingRequest,
    ) -> Result<GetTenantBrandingResponse, StatusError> {
        let code = req.code.trim();
        if code.is_empty() {
            return Ok(GetTenantBrandingResponse::default());
        }
        let tenant = tenants::Entity::find()
            .filter(tenants::Column::Code.eq(code))
            .one(&self.state.db)
            .await
            .unwrap_or(None);
        match tenant {
            Some(t) => Ok(GetTenantBrandingResponse {
                found: true,
                name: t.name,
                logo_url: t.logo_url.unwrap_or_default(),
            }),
            None => Ok(GetTenantBrandingResponse::default()),
        }
    }
}

/// code → access_token at the IdP token endpoint (client_secret_post).
async fn exchange_code(
    token_endpoint: &str,
    cfg: &SsoConfig,
    code: &str,
) -> Result<String, StatusError> {
    #[derive(serde::Deserialize)]
    struct TokenResp {
        access_token: String,
    }
    let resp = SSO_HTTP
        .post(token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", cfg.redirect_url.as_str()),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let token: TokenResp = r
                .json()
                .await
                .map_err(|_| status_error("UNAUTHORIZED", "sso code exchange failed"))?;
            if token.access_token.is_empty() {
                return Err(status_error("UNAUTHORIZED", "sso code exchange failed"));
            }
            Ok(token.access_token)
        }
        _ => Err(status_error("UNAUTHORIZED", "sso code exchange failed")),
    }
}

/// userinfo claims, flattened to strings (nested claims drop, matching
/// the reference's flat map).
async fn fetch_userinfo(
    userinfo_endpoint: &str,
    access_token: &str,
) -> Result<HashMap<String, String>, StatusError> {
    let resp = SSO_HTTP
        .get(userinfo_endpoint)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| status_error("UNAUTHORIZED", "sso userinfo failed"))?;
    if !resp.status().is_success() {
        return Err(status_error("UNAUTHORIZED", "sso userinfo failed"));
    }
    let raw: HashMap<String, serde_json::Value> = resp
        .json()
        .await
        .map_err(|_| status_error("UNAUTHORIZED", "sso userinfo failed"))?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k, s)),
            serde_json::Value::Number(n) => Some((k, n.to_string())),
            serde_json::Value::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect())
}

/// Minimal percent-encoding for the query values the auth URL carries.
fn urlencoding(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_enabled_requires_all_four_base_values() {
        let base = |issuer: &str, client: &str, secret: &str, redirect: &str| SsoConfig {
            issuer: issuer.into(),
            client_id: client.into(),
            client_secret: secret.into(),
            redirect_url: redirect.into(),
            scopes: String::new(),
            auto_create: false,
            tenant_code: String::new(),
        };
        assert!(base("https://idp", "cid", "sec", "https://app/cb").enabled());
        // Any empty leg closes the capability.
        assert!(!base("", "cid", "sec", "https://app/cb").enabled());
        assert!(!base("https://idp", "", "sec", "https://app/cb").enabled());
        assert!(!base("https://idp", "cid", "", "https://app/cb").enabled());
        assert!(!base("https://idp", "cid", "sec", "").enabled());
    }

    #[test]
    fn truthy_env_accepts_the_reference_set() {
        for v in ["1", "true", "yes", "on", "TRUE", " Yes "] {
            assert!(truthy_env(v), "{v} should be truthy");
        }
        for v in ["", "0", "false", "off", "enabled"] {
            assert!(!truthy_env(v), "{v} should not be truthy");
        }
    }

    #[test]
    fn urlencoding_escapes_query_semantics() {
        assert_eq!(urlencoding("openid email"), "openid%20email");
        assert_eq!(urlencoding("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencoding("state-1_2.3~"), "state-1_2.3~");
    }
}
