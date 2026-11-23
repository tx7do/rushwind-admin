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

/// The gate's verify face (signature + session validity) reused by the
/// edge's request-body guard — "has a valid access token" ahead of any
/// body read, same primitives as the gate, no second truth.
pub use rushwind_authn_gate::authenticate_and_check_session;

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
    let mut claims = match rushwind_authn_gate::authenticate_and_check_session(
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

    // 代租户操作（`X-Tenant-Id`）：平台管理员可携带该头把本请求的生效
    // 租户切到所选租户——门对该租户跑完租户阶段（存在/启用/到期/套餐
    // 白名单，与租户成员同一标准），随后把 claims 的 tid 覆写为生效租户：
    // 下游 Viewer 的租户视图、建行的租户归属、/me 归属全部按所选租户行事，
    // 而 token 未动——审计里的 uid/tid 仍是真实操作者（平台侧）。头缺失
    // 即平台全域视图（现状）；租户成员携带该头一律忽略（其租户由 token
    // 决定）；非平台管理员（tid=0 但无 platform:admin 角色）同样忽略。
    let token_tenant_id = claims.0.get("tid").and_then(|v| v.as_i64()).unwrap_or(0);
    let is_platform_admin = roles.iter().any(|r| r == "platform:admin");
    let impersonated = if token_tenant_id == 0 && is_platform_admin {
        parse_impersonated_tenant(
            req.headers()
                .get("x-tenant-id")
                .and_then(|v| v.to_str().ok()),
        )
    } else {
        None
    };
    let tenant_id = impersonated.unwrap_or(token_tenant_id);

    // The tenant stage: tenant members, and platform admins operating
    // inside an impersonated tenant.
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

    if let Some(effective) = impersonated {
        claims.0.insert("tid".into(), serde_json::json!(effective));
    }

    req.extensions_mut().insert(claims);
    next.run(req).await
}

/// The `X-Tenant-Id` impersonation header's parse: a positive tenant id
// makes the request; anything else (absent, non-numeric, zero,
// negative) means no impersonation — lenient by design, the header is
// an operator convenience and a garbage value must not turn into a
// different identity.
fn parse_impersonated_tenant(raw: Option<&str>) -> Option<i64> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse::<i64>().ok().filter(|v| *v > 0)
}

/// The gate's failure statuses, pinned as literals: the message texts
/// AND these numbers are the wire contract (the differential harness
/// byte-compares the 401 envelope). The admin contract's error table
/// anchors the same pair — `apps/admin/tests/admin_error_statuses.rs`
/// keeps that equivalence checked, so the framework crate stays free of
/// a reverse dependency on the contract.
const UNAUTHORIZED_STATUS: i32 = 401;
const FORBIDDEN_STATUS: i32 = 403;

/// The middleware failure: `UNAUTHORIZED` (401) with the stage
/// failure's fixed message text.
fn unauthorized(err: SessionError) -> rushwind_http_binding::envelope::StatusError {
    rushwind_http_binding::envelope::StatusError::new(
        UNAUTHORIZED_STATUS,
        "UNAUTHORIZED",
        err.message(),
    )
}

/// The tenant/authorization stage failure: `FORBIDDEN` (403) with the
/// check's own message text.
fn forbidden(message: &str) -> rushwind_http_binding::envelope::StatusError {
    rushwind_http_binding::envelope::StatusError::new(FORBIDDEN_STATUS, "FORBIDDEN", message)
}

#[cfg(test)]
mod tests {
    use super::parse_impersonated_tenant;

    /// 头解析：正整数十进制生效；缺失/空白/非数字/零/负数一律不构成
    /// 代租户——宽松失败，垃圾值不得变成另一个身份。
    #[test]
    fn impersonated_tenant_header_parses_strictly_positive() {
        assert_eq!(parse_impersonated_tenant(Some("42")), Some(42));
        assert_eq!(parse_impersonated_tenant(Some(" 7 ")), Some(7));
        assert_eq!(parse_impersonated_tenant(Some("0")), None);
        assert_eq!(parse_impersonated_tenant(Some("-3")), None);
        assert_eq!(parse_impersonated_tenant(Some("abc")), None);
        assert_eq!(parse_impersonated_tenant(Some("")), None);
        assert_eq!(parse_impersonated_tenant(Some("1.5")), None);
        assert_eq!(parse_impersonated_tenant(None), None);
    }
}
