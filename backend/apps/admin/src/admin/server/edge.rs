//! The reusable route-pack edge: everything a route pack mounts sits
//! on — the per-route layer composition, the dual-bucket mount loop,
//! the docs surface, and the audit-write layer — parameterized over
//! the pack's contract, so a business module's own route pack rides
//! the exact stack the admin surface rides (see [`super::rest`] for
//! the admin face over this module).
//!
//! Per-route layer composition ([`make_wrap`]): the framework bind
//! layer — body and query binding — outermost on EVERY route, and the
//! auth gate (the auth crate) composed INSIDE it on gated routes
//! only. That order is the wire contract: codec/binding failures
//! answer 400 ahead of any 401.
//!
//! The CORS policy and the request budget are NOT here: they ride the
//! assembler's per-server edge block and wrap every route pack on that
//! server identically. The request-body guard ([`body_guard`]) IS here:
//! the outermost app-owned layer, judging 413/401 before any body read.

use std::sync::Arc;

use axum::routing::MethodRouter;
use http_body_util::Limited;
use prost_reflect::DescriptorPool;

use crate::admin::state::AppState;
use auth::auth_gate;
use rushwind_http_binding::bindgate::bind_run;
use rushwind_http_binding::wire::RouteWire;

/// The wrap handshake the generated mounts consume (the generator
/// emits `&dyn Fn(MethodRouter, &RouteWire, bool) -> MethodRouter`).
pub type Wrap = dyn Fn(MethodRouter, &RouteWire, bool) -> MethodRouter;

/// One mounted service: threads both route buckets through the
/// generated mount with the service instance sealed inside.
pub type MountItem =
    Box<dyn Fn(axum::Router, axum::Router, &Wrap) -> (axum::Router, axum::Router) + Send + Sync>;

/// One route pack's edge configuration: everything [`edge_router`]
/// needs beyond the shared state.
pub struct EdgePack {
    /// The pack's annotated contract pool: the bind layer resolves
    /// request bindings against it.
    pub pool: &'static DescriptorPool,
    /// The registered codec subtypes — the packages the binary imports
    /// (the compatibility spec §2.1 register): the bind layer's
    /// Content-Type whitelist.
    pub subtypes: &'static [&'static str],
    /// The mounted services, one item per generated mount.
    pub mounts: Vec<MountItem>,
    /// The docs face (Swagger UI / Redoc / the raw spec mount),
    /// switched by this pack's settings node; `None` mounts no docs
    /// surface under this pack.
    pub docs: Option<crate::admin::server::docs::Wire>,
    /// The audit face (route table + operation skip lists) for the
    /// audit-write layer; `None` skips the layer. Shared, not owned:
    /// the admin face is a process-global and a module face is five
    /// rows — cloning route tables into every pack build is waste the
    /// Arc removes.
    pub audit: Option<std::sync::Arc<crate::admin::audit::AuditFace>>,
    /// Extra public routers merged ahead of the audit layer (the
    /// image-proxy style raw routes).
    pub extras: Vec<axum::Router>,
}

/// The adapter wiring the service-side Redis session store into the
/// auth gate's server-side check stage.
struct RedisTokenChecker(crate::admin::token::TokenStore);

#[async_trait::async_trait]
impl auth::AccessTokenChecker for RedisTokenChecker {
    async fn is_valid_access_token(&self, uid: i64, jti: &str, token: &str) -> bool {
        self.0.is_valid_access_token(uid, jti, token).await
    }
    async fn is_blocked_access_token(&self, jti: &str) -> bool {
        self.0.is_blocked_access_token(jti).await
    }
}

/// The unauthenticated request-body ceiling: every auth-free endpoint
/// (login, captcha, forgot-password, SSO exchange) answers well under
/// it, so an oversized declared body without a token is a probe, not
/// traffic — reject before reading.
const MAX_UNAUTH_BODY: u64 = 1 << 20;

/// The whole-site request-body ceiling: the base64-inflated 50MiB
/// upload (the transfer face carries file bytes as base64 inside the
/// JSON envelope) plus JSON structure headroom.
const MAX_REQUEST_BODY: u64 = crate::admin::services::MAX_UPLOAD_SIZE as u64 * 4 / 3 + (4 << 20);

/// The body-guard verdict for one request, decided from the declared
/// length and the token check alone (the pure face the wire layer
/// consumes): reject with an envelope error before any read, or the
/// byte ceiling the (possibly lying) stream is held to.
enum BodyDecision {
    Reject(rushwind_http_binding::envelope::StatusError),
    Limit(u64),
}

fn body_decision(content_length: Option<u64>, token_ok: bool) -> BodyDecision {
    match content_length {
        Some(len) if len > MAX_REQUEST_BODY => BodyDecision::Reject(
            crate::admin::state::status_error("PAYLOAD_TOO_LARGE", "request entity too large"),
        ),
        Some(len) if len <= MAX_UNAUTH_BODY => BodyDecision::Limit(MAX_UNAUTH_BODY),
        // 大 body 或长度未知（chunked）：先认证再放行读取——免鉴权端点
        // 没有大 body 的需求，无令牌的大请求是内存打爆面（DoS）。
        _ if token_ok => BodyDecision::Limit(MAX_REQUEST_BODY),
        _ => BodyDecision::Reject(crate::admin::state::status_error(
            "UNAUTHORIZED",
            "missing or invalid bearer token",
        )),
    }
}

/// GET/HEAD/OPTIONS 不携带请求体语义：Content-Length 缺席是常态而非
/// chunked 信号。守卫对这些方法免认证判定、按免鉴权小上限放行——此前
/// 无声明长度一律要令牌，无 body 的免鉴权 GET（验证码/openapi/图片代理）
/// 全被 401 打死（线上探针实证）。参照基座守卫本就只判 413 不做认证
/// （Go 侧 bodyless GET 的 ContentLength 是 0，恒过声明闸）。
fn bodyless_method(method: &axum::http::Method) -> bool {
    method == axum::http::Method::GET
        || method == axum::http::Method::HEAD
        || method == axum::http::Method::OPTIONS
}

/// The whole-site body guard (upstream 522d7ec9d 同语义): mounted
/// OUTSIDE every route (outermost of the edge stack, above the audit
/// layer), so the 413/401 verdicts land before the generated codec
/// reads the body — the codec's full-read happens inside the handler,
/// behind every layer mounted per-route, and a cap placed there could
/// never fire first. 401s skip the audit layer by design: transport
/// rejections are not audited operations.
async fn body_guard(
    auth: Arc<dyn rushwind_authn::Authenticator>,
    checker: Arc<dyn auth::AccessTokenChecker + 'static>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let content_length = req
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let verdict = match content_length {
        // Declared oversized: the verdict needs no token check.
        Some(len) if len > MAX_REQUEST_BODY => body_decision(Some(len), false),
        // Bodyless methods with no declared length: nothing to guard.
        None if bodyless_method(req.method()) => BodyDecision::Limit(MAX_UNAUTH_BODY),
        // Oversized-or-unknown: the token decides.
        _ => {
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
            let token_ok = auth::authenticate_and_check_session(
                auth.as_ref(),
                checker.as_ref(),
                &headers,
                bearer.as_deref(),
            )
            .await
            .is_ok();
            body_decision(content_length, token_ok)
        }
    };
    match verdict {
        BodyDecision::Reject(status) => rushwind_http_binding::envelope::error_response(status),
        BodyDecision::Limit(limit) => {
            // Content-Length can lie and chunked streams have none —
            // hold the actual stream to the ceiling too. The declared
            // path's limit is hyper-enforced; the wrap costs the
            // unknown-length path nothing but its bound.
            let (parts, body) = req.into_parts();
            let body = axum::body::Body::new(Limited::new(
                body,
                usize::try_from(limit).unwrap_or(usize::MAX),
            ));
            next.run(axum::extract::Request::from_parts(parts, body))
                .await
        }
    }
}

/// The per-route layer composition: the auth gate inside (gated routes
/// only), the bind layer outermost — the wire order, not a preference.
pub fn make_wrap(
    state: &Arc<AppState>,
    pool: &'static DescriptorPool,
    subtypes: &'static [&'static str],
) -> impl Fn(MethodRouter, &RouteWire, bool) -> MethodRouter + Send + Sync {
    let authenticator = Arc::clone(&state.authenticator);
    let checker = Arc::new(RedisTokenChecker(state.tokens.clone()))
        as Arc<dyn auth::AccessTokenChecker + 'static>;
    let tenant_checker = Arc::new(crate::admin::authorizer::TenantAccessChecker(Arc::clone(
        state,
    ))) as Arc<dyn auth::TenantAccessChecker + 'static>;
    // The authorization stage rides only when a configured engine kind
    // exists — `None` (section missing / `zanzibar`) drops the stage
    // the way the reference's nil engine drops its middleware.
    let authorizer: Option<Arc<dyn auth::AuthorizationEvaluator + 'static>> =
        state.cfg.authz_kind.map(|_| {
            Arc::new(crate::admin::authorizer::AccessAuthorizer::new(state))
                as Arc<dyn auth::AuthorizationEvaluator + 'static>
        });

    move |mr: MethodRouter, wire: &RouteWire, gated: bool| -> MethodRouter {
        let mut out = mr;
        if gated {
            let auth = Arc::clone(&authenticator);
            let checker = Arc::clone(&checker);
            let tenant_checker = Arc::clone(&tenant_checker);
            let authorizer = authorizer.clone();
            let gate = axum::middleware::from_fn(move |req, next| {
                let auth = Arc::clone(&auth);
                let checker = Arc::clone(&checker);
                let tenant_checker = Arc::clone(&tenant_checker);
                let authorizer = authorizer.clone();
                async move { auth_gate(auth, checker, tenant_checker, authorizer, req, next).await }
            });
            out = out.layer(gate);
        }
        let input_fq = wire.input_fq;
        let body_star = wire.body_star;
        let bind = axum::middleware::from_fn(move |req, next| {
            let (fq, body) = (input_fq, body_star);
            async move { bind_run(pool, fq, body, subtypes, req, next).await }
        });
        out = out.layer(bind);
        out
    }
}

/// Builds the mounted router. The assembly order mirrors the admin
/// face it was extracted from: buckets, merge, docs, extras, audit,
/// and the body guard outermost — the audit layer sees final status
/// codes, and the guard's 413/401 land before any body read.
pub fn edge_router(state: &Arc<AppState>, pack: EdgePack) -> axum::Router {
    let wrap = make_wrap(state, pack.pool, pack.subtypes);

    // The mounted surface. Each mount's generated auth-free table
    // classifies its route bindings into the two buckets.
    let mut router_pub = axum::Router::new();
    let mut router_gate = axum::Router::new();
    for mount in pack.mounts {
        (router_pub, router_gate) = mount(router_pub, router_gate, &wrap);
    }

    let mut app = router_pub.merge(router_gate);

    if let Some(docs) = pack.docs {
        app = app.merge(crate::admin::server::docs::router(
            docs.enable_swagger,
            docs.enable_redoc,
        ));
    }
    for extra in pack.extras {
        app = app.merge(extra);
    }

    // The audit-write layer: post-handler persistence into the audit
    // tables, outermost so it sees final status codes.
    if let Some(audit) = pack.audit {
        let state = Arc::clone(state);
        app = app.layer(axum::middleware::from_fn(move |req, next| {
            let state = Arc::clone(&state);
            let audit = Arc::clone(&audit);
            async move { crate::admin::audit::run(audit, state, req, next).await }
        }));
    }

    // The whole-site request-body guard: outermost of the edge stack —
    // the 413/401 verdicts must land before the codec (inside the
    // handler) reads the body, and unauthenticated oversized bodies are
    // exactly the pre-auth DoS face the guard exists to close.
    let guard_auth = Arc::clone(&state.authenticator);
    let guard_checker: Arc<dyn auth::AccessTokenChecker + 'static> =
        Arc::new(RedisTokenChecker(state.tokens.clone()));
    app = app.layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let auth = Arc::clone(&guard_auth);
            let checker = Arc::clone(&guard_checker);
            async move { body_guard(auth, checker, req, next).await }
        },
    ));

    app
}

/// The pack shell over the assembler handshake: the settings node
/// arrives per pack, and the builder turns it into the mounted router.
pub fn pack_with<F>(
    build: F,
) -> impl Fn(
    serde_json::Value,
    rushwind_bootstrap::RouteInput,
) -> Result<rushwind_bootstrap::RouteSurface, rushwind_bootstrap::BootstrapError>
       + Send
       + Sync
       + 'static
where
    F: Fn(serde_json::Value) -> Result<axum::Router, rushwind_bootstrap::BootstrapError>
        + Send
        + Sync
        + 'static,
{
    move |settings, _input| Ok(rushwind_bootstrap::RouteSurface::new(build(settings)?))
}

#[cfg(test)]
mod body_guard_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;

    /// The stub engine: no credential ever validates — the guard's
    /// no-token paths must answer from the header facts alone.
    struct NoAuth;
    #[async_trait::async_trait]
    impl rushwind_authn::Authenticator for NoAuth {
        fn scheme(&self) -> &'static str {
            rushwind_authn::SCHEME_BEARER
        }
        fn authenticate_token(
            &self,
            _token: &str,
        ) -> Result<rushwind_authn::AuthClaims, rushwind_authn::AuthnError> {
            Err(rushwind_authn::AuthnError::InvalidToken)
        }
        fn create_identity(
            &self,
            _claims: &rushwind_authn::AuthClaims,
        ) -> Result<String, rushwind_authn::AuthnError> {
            Err(rushwind_authn::AuthnError::InvalidToken)
        }
    }

    fn probe_router() -> axum::Router {
        let auth: Arc<dyn rushwind_authn::Authenticator> = Arc::new(NoAuth);
        let checker: Arc<dyn auth::AccessTokenChecker + 'static> = Arc::new(ProbeChecker);
        axum::Router::new()
            .route(
                "/probe",
                post(|| async { axum::Json(serde_json::json!({"ok": true})) }),
            )
            .layer(axum::middleware::from_fn(
                move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let auth = Arc::clone(&auth);
                    let checker = Arc::clone(&checker);
                    async move { body_guard(auth, checker, req, next).await }
                },
            ))
    }

    struct ProbeChecker;
    #[async_trait::async_trait]
    impl auth::AccessTokenChecker for ProbeChecker {
        async fn is_valid_access_token(&self, _uid: i64, _jti: &str, _token: &str) -> bool {
            false
        }
        async fn is_blocked_access_token(&self, _jti: &str) -> bool {
            false
        }
    }

    async fn post_with(headers: &[(&str, &str)], body: &'static str) -> axum::response::Response {
        let mut builder = Request::builder().method("POST").uri("/probe");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        probe_router()
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    /// 无 body 的免鉴权 GET（验证码/openapi 形态）：无 Content-Length、
    /// 无令牌，必须放行——此前长度未知一律要令牌，登录页第一跳就被
    /// 401 打死（线上探针实证的回归面）。
    #[tokio::test]
    async fn bodyless_get_without_token_passes_the_guard() {
        let auth: Arc<dyn rushwind_authn::Authenticator> = Arc::new(NoAuth);
        let checker: Arc<dyn auth::AccessTokenChecker + 'static> = Arc::new(ProbeChecker);
        let app = axum::Router::new()
            .route(
                "/probe",
                axum::routing::get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
            )
            .layer(axum::middleware::from_fn(
                move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let auth = Arc::clone(&auth);
                    let checker = Arc::clone(&checker);
                    async move { body_guard(auth, checker, req, next).await }
                },
            ));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn decision_pins_the_wire_contract() {
        // 声明超全站上限：413，不看令牌。
        assert!(matches!(
            body_decision(Some(MAX_REQUEST_BODY + 1), true),
            BodyDecision::Reject(_)
        ));
        // 小 body 免鉴权放行（登录/验证码/找回）。
        assert!(matches!(
            body_decision(Some(MAX_UNAUTH_BODY), false),
            BodyDecision::Limit(MAX_UNAUTH_BODY)
        ));
        // 1MiB 超额且无令牌：401。
        assert!(matches!(
            body_decision(Some(MAX_UNAUTH_BODY + 1), false),
            BodyDecision::Reject(_)
        ));
        // 1MiB 超额且有有效令牌：放行到全站上限。
        assert!(matches!(
            body_decision(Some(MAX_UNAUTH_BODY + 1), true),
            BodyDecision::Limit(MAX_REQUEST_BODY)
        ));
        // 长度未知（chunked）：无令牌 401，有令牌放行。
        assert!(matches!(
            body_decision(None, false),
            BodyDecision::Reject(_)
        ));
        assert!(matches!(
            body_decision(None, true),
            BodyDecision::Limit(MAX_REQUEST_BODY)
        ));
    }

    #[tokio::test]
    async fn oversized_declared_body_answers_413_before_any_read() {
        let len = (MAX_REQUEST_BODY + 1).to_string();
        let resp = post_with(&[("content-length", &len)], "{}").await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("PAYLOAD_TOO_LARGE"),
            "envelope reason missing: {text}"
        );
    }

    #[tokio::test]
    async fn oversized_unauthenticated_body_answers_401() {
        let len = (MAX_UNAUTH_BODY + 1).to_string();
        let resp = post_with(&[("content-length", &len)], "{}").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("UNAUTHORIZED"),
            "envelope reason missing: {text}"
        );
    }

    #[tokio::test]
    async fn chunked_unknown_length_without_token_answers_401() {
        let resp = post_with(&[], "{}").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn small_body_passes_through_the_limit_wrap() {
        let resp = post_with(&[("content-length", "2")], "{}").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true));
    }
}
