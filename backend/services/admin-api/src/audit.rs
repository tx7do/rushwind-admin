//! The audit-write layer (the
//! applogging.Server wrapper equivalent): post-handler persistence into the six
//! audit tables.
//!
//! Trigger rules:
//! * login audit — the face's login operations only, with the stateless
//!   risk heuristics;
//! * api audit — every request except the face's api-audit skips;
//! * operation audit — write methods only, session-maintenance
//!   operations skipped (`sessionOnlyOperations`);
//! * permission audit — write methods only, same session skips, target
//!   from the operation's service part, action from the method part.
//!
//! The layer is parameterized by an [`AuditFace`] — the route table it
//! resolves operations against plus the operation skip lists — so a
//! business route pack audits its own operations into the same tables
//! ([`AuditFace::admin`] is the admin face over the admin contract).
//!
//! The JWT is parsed from the bearer token directly (the
//! `extractAuthToken` path) so the outer layer needs no inner state.
//! Audit failures never break the response.

use std::sync::Arc;

use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use sea_orm::ActiveModelTrait;
use sea_orm::Set;

use crate::state::AppState;

mod risk;

/// One uniform audit route row: the only fields the audit layer reads
/// off a contract's route table (each contract crate emits its own
/// nominal route-spec type; this is the shared shape).
#[derive(Clone, Copy)]
pub struct AuditRoute {
    pub method: &'static str,
    pub path: &'static str,
    pub operation_id: &'static str,
}

/// One route pack's audit face: the route table the layer resolves
/// operations against, plus the operation skip lists.
#[derive(Clone)]
pub struct AuditFace {
    routes: Vec<AuditRoute>,
    login_ops: Vec<&'static str>,
    skip_api_audit: Vec<&'static str>,
    session_only: Vec<&'static str>,
}

impl AuditFace {
    /// Builds a face over `(method, path, operation_id)` rows with
    /// empty skip lists — a business pack's typical shape.
    pub fn from_rows(rows: &[(&'static str, &'static str, &'static str)]) -> Self {
        Self {
            routes: rows
                .iter()
                .map(|(method, path, operation_id)| AuditRoute {
                    method,
                    path,
                    operation_id,
                })
                .collect(),
            login_ops: Vec::new(),
            skip_api_audit: Vec::new(),
            session_only: Vec::new(),
        }
    }

    /// The admin face: the admin contract's route table plus the
    /// login-family skip lists.
    pub fn admin() -> &'static Self {
        static FACE: std::sync::OnceLock<AuditFace> = std::sync::OnceLock::new();
        FACE.get_or_init(|| {
            let mut face = Self::from_rows(
                &proto::gen::routes::ROUTES
                    .iter()
                    .map(|spec| (spec.method, spec.path, spec.operation_id))
                    .collect::<Vec<_>>(),
            );
            face.login_ops = LOGIN_OPS.to_vec();
            face.skip_api_audit = SKIP_API_AUDIT.to_vec();
            face.session_only = SESSION_ONLY.to_vec();
            face
        })
    }
}

/// The admin face's login-family operation skip lists.
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

/// Resolves the operation id by matching the face's route table.
fn resolve_operation(method: &Method, path: &str, face: &AuditFace) -> &'static str {
    for spec in &face.routes {
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
        // 回收站两动词（issue #3）：restore/purge 不落 OTHER。
        "RestoreTenant" => "RESTORE",
        "PurgeTenant" => "PURGE",
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

/// The axum middleware body over one audit face: resolves operations
/// against the face's route table and applies the face's skip lists.
/// The face rides the spawn as an owned handle — the audit write is
/// best-effort and must not tie the response to the borrow.
pub async fn run(
    face: Arc<AuditFace>,
    state: Arc<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let started = std::time::Instant::now();

    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let operation = resolve_operation(&method, &path, &face);
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

        // ---- login audit (the face's login operations) ----
        if face.login_ops.contains(&operation) {
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

        // ---- api audit (all except the face's api-audit skips) ----
        if !face.skip_api_audit.contains(&operation) {
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
        if is_write && !face.session_only.contains(&operation) && !operation.is_empty() {
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
                    // 回收站两动词（issue #3）。
                    "RestoreTenant" => "RESTORE",
                    "PurgeTenant" => "PURGE",
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    /// A URL-safe unpadded bearer JWT whose payload carries the given
    /// claim JSON (signature unchecked — attribution only).
    fn bearer_with_claims(json: &str) -> HeaderMap {
        use base64::Engine as _;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer header.{payload}.signature")
                .parse()
                .unwrap(),
        );
        headers
    }

    // ---- path_matches ----

    #[test]
    fn path_exact_and_template_match() {
        assert!(path_matches("/admin/v1/users", "/admin/v1/users"));
        assert!(path_matches("/admin/v1/users/{id}", "/admin/v1/users/42"));
        assert!(path_matches(
            "/admin/v1/users/{id}/roles/{rid}",
            "/admin/v1/users/42/roles/7"
        ));
        // Leading/trailing slashes are insignificant.
        assert!(path_matches("/admin/v1/users", "/admin/v1/users/"));
        assert!(path_matches("admin/v1/users", "/admin/v1/users"));
    }

    #[test]
    fn path_template_mismatches() {
        // A literal segment must match exactly.
        assert!(!path_matches("/admin/v1/users", "/admin/v1/roles"));
        // Segment count must match.
        assert!(!path_matches("/admin/v1/users/{id}", "/admin/v1/users"));
        assert!(!path_matches("/admin/v1/users", "/admin/v1/users/42"));
        // A template segment is literal, not a wildcard.
        assert!(!path_matches("/admin/v1/users/{id}", "/admin/v1/roles/42"));
    }

    // ---- resolve_operation ----

    #[test]
    fn resolves_against_the_admin_face_route_table() {
        let face = AuditFace::admin();
        assert_eq!(
            resolve_operation(&Method::GET, "/admin/v1/access-keys", face),
            "/admin.service.v1.AccessKeyService/List"
        );
        assert_eq!(
            resolve_operation(&Method::GET, "/admin/v1/access-keys/42", face),
            "/admin.service.v1.AccessKeyService/Get"
        );
        assert_eq!(
            resolve_operation(&Method::POST, "/admin/v1/access-keys", face),
            "/admin.service.v1.AccessKeyService/Create"
        );
    }

    #[test]
    fn resolve_unknown_route_and_method_miss() {
        let face = AuditFace::admin();
        assert_eq!(
            resolve_operation(&Method::GET, "/admin/v1/no-such-thing", face),
            ""
        );
        // The template only matches its own method.
        assert_eq!(
            resolve_operation(&Method::PATCH, "/admin/v1/access-keys/42", face),
            ""
        );
    }

    // ---- request_id ----

    #[test]
    fn request_id_priority_order() {
        let mut headers = HeaderMap::new();
        headers.insert("x-correlation-id", "corr".parse().unwrap());
        headers.insert("x-request-id", "req".parse().unwrap());
        assert_eq!(request_id(&headers), "req");

        headers.remove("x-request-id");
        assert_eq!(request_id(&headers), "corr");

        headers.insert("x-fc-request-id", "fc".parse().unwrap());
        headers.remove("x-correlation-id");
        assert_eq!(request_id(&headers), "fc");
    }

    #[test]
    fn request_id_skips_empty_values_and_falls_back_to_uuid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "".parse().unwrap());
        let fallback = request_id(&headers);
        // A v4 UUID keeps the version nibble and the hyphen layout.
        assert_eq!(fallback.len(), 36);
        assert_eq!(fallback.as_bytes()[14], b'4');
    }

    // ---- claims_from_token ----

    #[test]
    fn claims_decode_uid_tid_sub() {
        let headers = bearer_with_claims(r#"{"uid":7,"tid":3,"sub":"admin"}"#);
        assert_eq!(
            claims_from_token(&headers),
            Some((7, 3, "admin".to_string()))
        );
    }

    #[test]
    fn claims_default_missing_tid_and_sub() {
        let headers = bearer_with_claims(r#"{"uid":9}"#);
        assert_eq!(claims_from_token(&headers), Some((9, 0, String::new())));
    }

    #[test]
    fn claims_none_without_bearer_or_payload() {
        // No Authorization header at all.
        assert_eq!(claims_from_token(&HeaderMap::new()), None);
        // Another scheme rides.
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(claims_from_token(&headers), None);
        // Missing the payload segment.
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer onlyheader".parse().unwrap());
        assert_eq!(claims_from_token(&headers), None);
        // Non-numeric uid.
        let headers = bearer_with_claims(r#"{"uid":"seven"}"#);
        assert_eq!(claims_from_token(&headers), None);
    }

    // ---- target_name_from_body ----

    #[test]
    fn target_name_priority_chain_inside_data() {
        // `name` leads the chain, ahead of title/code.
        assert_eq!(
            target_name_from_body(&Some(r#"{"data":{"title":"t","name":"n"}}"#.into())),
            Some("n".to_string())
        );
        assert_eq!(
            target_name_from_body(&Some(r#"{"data":{"code":"c"}}"#.into())),
            Some("c".to_string())
        );
    }

    #[test]
    fn target_name_falls_back_to_top_level_and_skips_empty() {
        // No `data` envelope: the root object is probed.
        assert_eq!(
            target_name_from_body(&Some(r#"{"nickname":"nick"}"#.into())),
            Some("nick".to_string())
        );
        // An empty name yields to the next key in the chain.
        assert_eq!(
            target_name_from_body(&Some(r#"{"data":{"name":"","realname":"real"}}"#.into())),
            Some("real".to_string())
        );
    }

    #[test]
    fn target_name_none_shapes() {
        assert_eq!(target_name_from_body(&None), None);
        assert_eq!(target_name_from_body(&Some("not-json".into())), None);
        assert_eq!(target_name_from_body(&Some(r#"{"data":{}}"#.into())), None);
        // A non-string name field is not a target.
        assert_eq!(
            target_name_from_body(&Some(r#"{"data":{"name":42}}"#.into())),
            None
        );
    }

    // ---- login_username ----

    #[test]
    fn login_username_body_wins_over_header() {
        assert_eq!(
            login_username(&Some(r#"{"username":"body"}"#.into()), "header"),
            "body"
        );
    }

    #[test]
    fn login_username_header_fallbacks() {
        // Invalid JSON.
        assert_eq!(login_username(&Some("not-json".into()), "header"), "header");
        // No username key.
        assert_eq!(
            login_username(&Some(r#"{"user":"x"}"#.into()), "header"),
            "header"
        );
        // No body at all.
        assert_eq!(login_username(&None, "header"), "header");
    }

    // ---- parse_target_and_action ----

    #[test]
    fn parses_target_and_every_action() {
        let cases = [
            ("/admin.service.v1.RoleService/Create", ("role", "CREATE")),
            (
                "/admin.service.v1.RoleService/BatchCreate",
                ("role", "CREATE"),
            ),
            ("/admin.service.v1.RoleService/Update", ("role", "UPDATE")),
            ("/admin.service.v1.RoleService/Delete", ("role", "DELETE")),
            (
                "/admin.service.v1.RoleService/BatchDelete",
                ("role", "DELETE"),
            ),
            ("/admin.service.v1.RoleService/Assign", ("role", "ASSIGN")),
            (
                "/admin.service.v1.RoleService/Unassign",
                ("role", "UNASSIGN"),
            ),
            ("/admin.service.v1.RoleService/Export", ("role", "OTHER")),
        ];
        for (operation, (target, action)) in cases {
            assert_eq!(
                parse_target_and_action(operation),
                Some((target.to_string(), action.to_string())),
                "operation {operation}"
            );
        }
    }

    #[test]
    fn parses_the_service_suffix_off_the_target() {
        // Not "v1role": the version segment is dropped with the rest of
        // the package path.
        assert_eq!(
            parse_target_and_action("/admin.service.v1.MfaService/Update"),
            Some(("mfa".to_string(), "UPDATE".to_string()))
        );
    }

    #[test]
    fn parse_rejects_malformed_operations() {
        // Trailing slash — no method part.
        assert_eq!(
            parse_target_and_action("/admin.service.v1.RoleService/"),
            None
        );
        // No dot in the service part.
        assert_eq!(parse_target_and_action("service/Create"), None);
        // No slash at all.
        assert_eq!(parse_target_and_action("noslash"), None);
    }

    // ---- last_numeric_segment ----

    #[test]
    fn last_numeric_segment_finds_the_resource_id() {
        assert_eq!(
            last_numeric_segment("/admin/v1/users/42"),
            Some("42".into())
        );
        assert_eq!(
            last_numeric_segment("/admin/v1/users/42/roles/7"),
            Some("7".into())
        );
        // A trailing slash does not hide the id.
        assert_eq!(
            last_numeric_segment("/admin/v1/users/42/"),
            Some("42".into())
        );
        // Scanning runs from the end: the trailing literal does not stop it.
        assert_eq!(
            last_numeric_segment("/admin/v1/tenants/7/members"),
            Some("7".into())
        );
    }

    #[test]
    fn last_numeric_segment_none_shapes() {
        // No numeric segment anywhere in the path.
        assert_eq!(last_numeric_segment("/admin/v1/users"), None);
        assert_eq!(last_numeric_segment("/admin/v1/users/me"), None);
        assert_eq!(last_numeric_segment("/admin/v1/users/me/settings"), None);
    }

    // ---- AuditFace ----

    #[test]
    fn from_rows_face_has_empty_skip_lists() {
        let face = AuditFace::from_rows(&[("GET", "/biz/v1/things", "/biz.service.v1.Thing/List")]);
        assert!(!face.login_ops.contains(&"/biz.service.v1.Thing/List"));
        assert!(!face.skip_api_audit.contains(&"/biz.service.v1.Thing/List"));
        assert!(!face.session_only.contains(&"/biz.service.v1.Thing/List"));
        assert_eq!(
            resolve_operation(&Method::GET, "/biz/v1/things", &face),
            "/biz.service.v1.Thing/List"
        );
    }

    #[test]
    fn admin_face_carries_the_login_family_skips() {
        let face = AuditFace::admin();
        let login = "/admin.service.v1.AuthenticationService/Login";
        let refresh = "/admin.service.v1.AuthenticationService/RefreshToken";
        assert!(face.login_ops.contains(&login));
        assert!(face.skip_api_audit.contains(&login));
        assert!(face.session_only.contains(&refresh));
        // The login and MFA-challenge operations are exactly the api-audit skips.
        for op in &face.skip_api_audit {
            assert!(
                face.login_ops.contains(op),
                "skip {op} must be a login operation"
            );
        }
    }
}
