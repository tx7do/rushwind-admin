//! The auth gate for the protected subtree — the deployment face. The
//! session stage (the checker contract, the authenticate-then-check
//! head, the fixed failure texts) lives in `rushwind-authn-gate`; this
//! file composes the admin's two later stages on top of the returned
//! claims and anchors the failure envelopes to the admin error tables.
//!
//! Stage order, in wire order: the framework session stage
//! (`Authenticator.Authenticate(ACCESS)` → Redis whitelist/blacklist)
//! → `TenantAccessChecker` (tenant status / expiry read-only / plan
//! module whitelist — tenant members only, platform admins skip) →
//! `AuthorizationEvaluator` (per-role evaluation with the audit
//! trail). Failures render the four-field status envelope: reason
//! `UNAUTHORIZED` for every credential failure (message pinned by the
//! framework's failure classes), reason `FORBIDDEN` with the check's
//! own message for the two later stages. Success inserts the claim bag
//! into the request extensions — the request-context injection —
//! consumed by the glue into the per-request
//! [`rushwind_http_binding::ctx::RequestContext`].

use std::sync::Arc;

use async_trait::async_trait;
use rushwind_authn::Authenticator;
use rushwind_authn_gate::SessionError;

pub use rushwind_authn_gate::AccessTokenChecker;

/// The tenant-level access checks — status, expiry read-only, and the
/// plan module whitelist — split out so the storage stays service-side.
/// Only tenant members (`tid > 0`) reach it; platform admins skip.
#[async_trait]
pub trait TenantAccessChecker: Send + Sync {
    /// `path` is the matched route template (the reference
    /// `PathTemplate()` form, `/admin/v1/users/{id}`), `method` the
    /// HTTP verb. `Err` carries the exact FORBIDDEN envelope text.
    async fn check_tenant_access(
        &self,
        tenant_id: i64,
        path: &str,
        method: &str,
    ) -> Result<(), String>;
}

/// The per-request inputs of the authorization evaluation.
pub struct AuthzEvaluation<'a> {
    pub user_id: i64,
    pub tenant_id: i64,
    /// The token's role codes — one evaluation per role, the first
    /// permitting role wins.
    pub roles: &'a [String],
    /// The HTTP verb.
    pub action: &'a str,
    /// The matched route template.
    pub resource: &'a str,
    /// Best-effort client IP (`X-Real-IP`, then the first
    /// `X-Forwarded-For` entry).
    pub ip: &'a str,
    /// `traceparent`'s trace id, else `X-Request-Id`.
    pub trace_id: &'a str,
}

/// The authorization point: evaluates the request against the policy
/// engine and records the evaluation trail. `Err` carries the exact
/// FORBIDDEN envelope text.
#[async_trait]
pub trait AuthorizationEvaluator: Send + Sync {
    async fn authorize(&self, evaluation: AuthzEvaluation<'_>) -> Result<(), String>;
}

/// The gate middleware body: the framework session stage, then the
/// tenant and authorization stages, pass through with the claims
/// injected, or render the envelope.
pub async fn auth_gate(
    auth: Arc<dyn Authenticator>,
    checker: Arc<dyn AccessTokenChecker + 'static>,
    tenant_checker: Arc<dyn TenantAccessChecker + 'static>,
    authorizer: Arc<dyn AuthorizationEvaluator + 'static>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let bearer = rushwind_http_binding::ctx::bearer_token(req.headers());
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let claims = match rushwind_authn_gate::authenticate_and_check_session(
        auth.as_ref(),
        checker.as_ref(),
        &headers,
        bearer.as_deref(),
    )
    .await
    {
        Ok(claims) => claims,
        Err(err) => {
            return rushwind_http_binding::envelope::error_response(unauthorized(err));
        }
    };

    // The request facts the later stages evaluate against: the
    // matched route template (the reference `PathTemplate()` form), the
    // verb, and the best-effort client headers.
    let path = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    let method = req.method().to_string();
    let ip = rushwind_http_binding::ctx::client_ip(
        req.headers(),
        rushwind_http_binding::ctx::IpSource::RealIpFirst,
    );
    let trace_id = rushwind_authn_gate::trace_id(req.headers());

    // The tenant stage: tenant members only.
    let tenant_id = claims.0.get("tid").and_then(|v| v.as_i64()).unwrap_or(0);
    if tenant_id > 0 {
        if let Err(message) = tenant_checker
            .check_tenant_access(tenant_id, &path, &method)
            .await
        {
            return rushwind_http_binding::envelope::error_response(forbidden(&message));
        }
    }

    // The authorization stage: one evaluation per role, the
    // first permitting role wins.
    let roles: Vec<String> = claims
        .0
        .get("roc")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let evaluation = AuthzEvaluation {
        user_id: claims.0.get("uid").and_then(|v| v.as_i64()).unwrap_or(0),
        tenant_id,
        roles: &roles,
        action: &method,
        resource: &path,
        ip: &ip,
        trace_id: &trace_id,
    };
    if let Err(message) = authorizer.authorize(evaluation).await {
        return rushwind_http_binding::envelope::error_response(forbidden(&message));
    }

    req.extensions_mut().insert(claims);
    next.run(req).await
}

/// The middleware failure: the status the admin error tables anchor to
/// `UNAUTHORIZED` (401), and the stage failure's fixed message text.
fn unauthorized(err: SessionError) -> rushwind_http_binding::envelope::StatusError {
    let status = proto::tables::error_status("admin.service.v1", "UNAUTHORIZED").unwrap_or(401);
    rushwind_http_binding::envelope::StatusError::new(status, "UNAUTHORIZED", err.message())
}

/// The tenant/authorization stage failure: `FORBIDDEN` (403) with the
/// check's own message text.
fn forbidden(message: &str) -> rushwind_http_binding::envelope::StatusError {
    let status = proto::tables::error_status("admin.service.v1", "FORBIDDEN").unwrap_or(403);
    rushwind_http_binding::envelope::StatusError::new(status, "FORBIDDEN", message)
}
