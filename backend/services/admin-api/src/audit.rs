//! The audit-write layer (the
//! applogging.Server wrapper equivalent): post-handler persistence into the six
//! audit tables.
//!
//! Trigger rules:
//! * login audit — Login / VerifyMFAChallenge / Logout operations only,
//!   with the stateless risk heuristics;
//! * api audit — every request except Login / VerifyMFAChallenge;
//! * operation audit — write methods only, session-maintenance
//!   operations skipped (`sessionOnlyOperations`);
//! * permission audit — write methods only, same session skips, target
//!   from the operation's service part, action from the method part.
//!
//! The JWT is parsed from the bearer token directly (the
//! `extractAuthToken` path) so the outer layer needs no inner state.
//! Audit failures never break the response.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use sea_orm::ActiveModelTrait;
use sea_orm::Set;

use crate::state::AppState;

mod risk;

/// The session-maintenance operation skip list.
const SESSION_ONLY: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.AuthenticationService/RefreshToken",
    "/admin.service.v1.AuthenticationService/Logout",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
];

const LOGIN_OPS: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
    "/admin.service.v1.AuthenticationService/Logout",
];

const SKIP_API_AUDIT: &[&str] = &[
    "/admin.service.v1.AuthenticationService/Login",
    "/admin.service.v1.MfaService/VerifyMFAChallenge",
];

const WRITE_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

const MAX_BODY_SNAPSHOT: usize = 64 << 10;

/// Matches a concrete request path against a route template (`{var}`
/// segments match any non-empty segment).
fn path_matches(template: &str, path: &str) -> bool {
    let t: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
    let p: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if t.len() != p.len() {
        return false;
    }
    t.iter()
        .zip(p.iter())
        .all(|(tt, pp)| tt.starts_with('{') || tt == pp)
}

/// Resolves the operation id by matching the static route table.
fn resolve_operation(method: &Method, path: &str) -> &'static str {
    for spec in proto::gen::routes::ROUTES {
        if spec.method == method.as_str() && path_matches(spec.path, path) {
            return spec.operation_id;
        }
    }
    ""
}

fn client_ip(headers: &axum::http::HeaderMap) -> String {
    rushwind_http_binding::ctx::client_ip(
        headers,
        rushwind_http_binding::ctx::IpSource::ForwardedFirst,
    )
}

fn request_id(headers: &axum::http::HeaderMap) -> String {
    for key in ["x-request-id", "x-correlation-id", "x-fc-request-id"] {
        if let Some(v) = headers.get(key).and_then(|v| v.to_str().ok()) {
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    uuid::Uuid::new_v4().to_string()
}

/// Decodes the bearer JWT payload without re-verification — attribution
/// only (gated routes already verified it inside the gate).
fn claims_from_token(headers: &axum::http::HeaderMap) -> Option<(u64, u64, String)> {
    use base64::Engine as _;
    let token = rushwind_http_binding::ctx::bearer_token(headers)?;
    let payload_b64 = token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let claims: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(&payload).ok()?).ok()?;
    let uid = claims.get("uid")?.as_u64()?;
    let tid = claims.get("tid").and_then(|v| v.as_u64()).unwrap_or(0);
    let username = claims
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Some((uid, tid, username))
}

/// The first non-empty name-ish field from the JSON body's `data` object
/// (permission audit target name).
fn target_name_from_body(body: &Option<String>) -> Option<String> {
    let body = body.as_ref()?;
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let data = value.get("data").unwrap_or(&value);
    for key in ["name", "title", "username", "nickname", "realname", "code"] {
        if let Some(name) = data.get(key).and_then(|v| v.as_str()) {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Username extraction for login audits: JSON body first, then the
/// X-Audit-Username response header (set by the MFA handler).
fn login_username(body: &Option<String>, audit_header: &str) -> String {
    if let Some(body) = body {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(name) = value.get("username").and_then(|v| v.as_str()) {
                return name.to_string();
            }
        }
    }
    audit_header.to_string()
}

/// Target/action parse from the operation string.
fn parse_target_and_action(operation: &str) -> Option<(String, String)> {
    let slash = operation.rfind('/')?;
    if slash == operation.len() - 1 {
        return None;
    }
    let service_part = &operation[..slash];
    let method = &operation[slash + 1..];
    let dot = service_part.rfind('.')?;
    let mut svc = &service_part[dot + 1..];
    svc = svc.strip_suffix("Service").unwrap_or(svc);
    let target = svc.to_lowercase();
    let action = match method {
        "Create" | "BatchCreate" => "CREATE",
        "Update" => "UPDATE",
        "Delete" | "BatchDelete" => "DELETE",
        "Assign" => "ASSIGN",
        "Unassign" => "UNASSIGN",
        _ => "OTHER",
    };
    Some((target, action.to_string()))
}

/// Last purely-numeric path segment (resource id).
fn last_numeric_segment(path: &str) -> Option<String> {
    path.split('/')
        .rev()
        .find(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .map(String::from)
}

/// The axum middleware body.
pub async fn layer(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let started = std::time::Instant::now();

    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let operation = resolve_operation(&method, &path);
    let ip = client_ip(req.headers());
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let referer = req
        .headers()
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let request_id = request_id(req.headers());
    let claims = claims_from_token(req.headers());

    // Body snapshot: write methods with a JSON content type, ≤ 64 KiB
    // (snapshotWriteBody). The buffered body rebuilds the request so the
    // inner chain reads it normally.
    let is_write = WRITE_METHODS.contains(&method.as_str());
    let is_json = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_lowercase().contains("application/json"));
    let mut body: Option<String> = None;
    let req = if is_write && is_json {
        let (parts, body_body) = req.into_parts();
        let bytes = axum::body::to_bytes(body_body, MAX_BODY_SNAPSHOT)
            .await
            .unwrap_or_default();
        if !bytes.is_empty() {
            body = String::from_utf8(bytes.to_vec()).ok();
        }
        Request::from_parts(parts, axum::body::Body::from(bytes))
    } else {
        req
    };

    let response = next.run(req).await;

    let latency_ms = started.elapsed().as_millis() as u32;
    let success = response.status() == StatusCode::OK;
    let status_code = response.status().as_u16() as u32;
    // The MFA handler's attribution header — read before the response
    // moves into the return.
    let audit_username_header = response
        .headers()
        .get("x-audit-username")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();

    // Audit writes are best-effort; failures never break the response.
    let db = state.db.clone();
    tokio::spawn(async move {
        let uid = claims.as_ref().map(|c| c.0).unwrap_or(0);
        let tid = claims.as_ref().map(|c| c.1).unwrap_or(0);
        let username = claims.as_ref().map(|c| c.2.clone()).unwrap_or_default();
        let created = crate::data::now();

        // ---- login audit (Login / VerifyMFAChallenge / Logout) ----
        if LOGIN_OPS.contains(&operation) {
            let is_logout = operation.ends_with("/Logout");
            let failed = !success;
            let login_username = if is_logout {
                username.clone()
            } else {
                login_username(&body, &audit_username_header)
            };
            let failure_reason = if failed {
                Some("login failed".to_string())
            } else {
                None
            };
            let has_device = !user_agent.is_empty();
            let score = risk::risk_score(failed, uid as i64, &login_username, &ip, has_device)
                .max(0) as u32;
            let factors = risk::risk_factors(
                failed,
                uid as i64,
                &login_username,
                &ip,
                has_device,
                "",
                failure_reason.as_deref().unwrap_or(""),
                &request_id,
                score,
            );
            let row = crate::data::sys_login_audit_logs::ActiveModel {
                tenant_id: Set(Some(tid as i64)),
                user_id: Set(Some(uid as i64)),
                username: Set(Some(login_username.clone())),
                ip_address: Set(Some(ip.clone())),
                request_id: Set(Some(request_id.clone())),
                action_type: Set(Some(if is_logout {
                    "LOGOUT".into()
                } else {
                    "LOGIN".into()
                })),
                status: Set(Some(if failed {
                    "FAILED".into()
                } else {
                    "SUCCESS".into()
                })),
                failure_reason: Set(failure_reason),
                login_method: Set(Some("PASSWORD".into())),
                risk_score: Set(Some(score)),
                risk_level: Set(Some(risk::risk_level(score).into())),
                risk_factors: Set(Some(serde_json::Value::Array(
                    factors
                        .iter()
                        .map(|f| serde_json::Value::String(f.clone()))
                        .collect(),
                ))),
                created_at: Set(Some(created)),
                ..Default::default()
            }
            .insert(&db)
            .await;
            if row.is_err() {
                eprintln!("[audit] login log insert failed");
            }
        }

        // ---- api audit (all except Login / VerifyMFAChallenge) ----
        if !SKIP_API_AUDIT.contains(&operation) {
            let row = crate::data::sys_api_audit_logs::ActiveModel {
                tenant_id: Set(Some(tid as i64)),
                user_id: Set(Some(uid as i64)),
                username: Set(Some(username.clone())),
                ip_address: Set(Some(ip.clone())),
                http_method: Set(Some(method.to_string())),
                path: Set(Some(
                    operation
                        .trim_start_matches('/')
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                )),
                request_uri: Set(Some(path.clone())),
                api_operation: Set(Some(operation.to_string())),
                request_id: Set(Some(request_id.clone())),
                latency_ms: Set(Some(latency_ms)),
                success: Set(Some(success)),
                status_code: Set(Some(status_code)),
                reason: Set((!success).then(|| format!("HTTP {status_code}"))),
                request_body: Set(body.clone()),
                referer: Set(Some(referer.clone())),
                created_at: Set(Some(created)),
                ..Default::default()
            }
            .insert(&db)
            .await;
            if row.is_err() {
                eprintln!("[audit] api log insert failed");
            }
        }

        // ---- operation audit (write methods, session ops skipped) ----
        // ---- permission audit (same skips; target/action parsed) ----
        if is_write && !SESSION_ONLY.contains(&operation) && !operation.is_empty() {
            let resource_id = last_numeric_segment(&path);
            let (target_type, perm_action) =
                parse_target_and_action(operation).unwrap_or((String::new(), "OTHER".into()));

            // Operation audit: resource type = service minus "Service".
            let resource_type = operation
                .trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or("")
                .rsplit('.')
                .next()
                .unwrap_or("")
                .strip_suffix("Service")
                .unwrap_or("")
                .to_lowercase();
            if !resource_type.is_empty() {
                let action = match operation.rsplit('/').next().unwrap_or("") {
                    "Create" | "BatchCreate" => "CREATE",
                    "Update" => "UPDATE",
                    "Delete" | "BatchDelete" => "DELETE",
                    "Export" => "EXPORT",
                    "Import" => "IMPORT",
                    "Assign" => "ASSIGN",
                    "Unassign" => "UNASSIGN",
                    _ => "OTHER",
                };
                let row = crate::data::sys_operation_audit_logs::ActiveModel {
                    tenant_id: Set(Some(tid as i64)),
                    user_id: Set(Some(uid as i64)),
                    username: Set(Some(username.clone())),
                    resource_type: Set(Some(resource_type.clone())),
                    resource_id: Set(resource_id.clone()),
                    action: Set(Some(action.into())),
                    request_id: Set(Some(request_id.clone())),
                    success: Set(Some(success)),
                    failure_reason: Set((!success).then(|| format!("HTTP {status_code}"))),
                    ip_address: Set(Some(ip.clone())),
                    created_at: Set(Some(created)),
                    ..Default::default()
                }
                .insert(&db)
                .await;
                if row.is_err() {
                    eprintln!("[audit] operation log insert failed");
                }
            }

            // Permission audit: needs a parsed target type.
            if !target_type.is_empty() {
                let target_name = target_name_from_body(&body);
                let row = crate::data::sys_permission_audit_logs::ActiveModel {
                    tenant_id: Set(Some(tid as i64)),
                    operator_id: Set(Some(uid as i64)),
                    operator_name: Set(Some(username.clone())),
                    target_type: Set(Some(target_type)),
                    target_id: Set(resource_id),
                    target_name: Set(target_name),
                    action: Set(Some(perm_action)),
                    ip_address: Set(Some(ip.clone())),
                    request_id: Set(Some(request_id.clone())),
                    created_at: Set(Some(created)),
                    ..Default::default()
                }
                .insert(&db)
                .await;
                if row.is_err() {
                    eprintln!("[audit] permission log insert failed");
                }
            }
        }
    });

    response
}
