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
//! server identically.

use std::sync::Arc;

use axum::routing::MethodRouter;
use prost_reflect::DescriptorPool;

use crate::state::AppState;
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
    pub docs: Option<crate::server::docs::Wire>,
    /// The audit face (route table + operation skip lists) for the
    /// audit-write layer; `None` skips the layer.
    pub audit: Option<crate::audit::AuditFace>,
    /// Extra public routers merged ahead of the audit layer (the
    /// image-proxy style raw routes).
    pub extras: Vec<axum::Router>,
}

/// The adapter wiring the service-side Redis session store into the
/// auth gate's server-side check stage.
struct RedisTokenChecker(crate::token::TokenStore);

#[async_trait::async_trait]
impl auth::AccessTokenChecker for RedisTokenChecker {
    async fn is_valid_access_token(&self, uid: i64, jti: &str, token: &str) -> bool {
        self.0.is_valid_access_token(uid, jti, token).await
    }
    async fn is_blocked_access_token(&self, jti: &str) -> bool {
        self.0.is_blocked_access_token(jti).await
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
    let tenant_checker = Arc::new(crate::authorizer::TenantAccessChecker(Arc::clone(state)))
        as Arc<dyn auth::TenantAccessChecker + 'static>;
    let authorizer = Arc::new(crate::authorizer::AccessAuthorizer::new(Arc::clone(state)))
        as Arc<dyn auth::AuthorizationEvaluator + 'static>;

    move |mr: MethodRouter, wire: &RouteWire, gated: bool| -> MethodRouter {
        let mut out = mr;
        if gated {
            let auth = Arc::clone(&authenticator);
            let checker = Arc::clone(&checker);
            let tenant_checker = Arc::clone(&tenant_checker);
            let authorizer = Arc::clone(&authorizer);
            let gate = axum::middleware::from_fn(move |req, next| {
                let auth = Arc::clone(&auth);
                let checker = Arc::clone(&checker);
                let tenant_checker = Arc::clone(&tenant_checker);
                let authorizer = Arc::clone(&authorizer);
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
/// face it was extracted from: buckets, merge, docs, extras, audit
/// outermost — the audit layer sees final status codes.
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
        app = app.merge(crate::server::docs::router(
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
        let audit = Arc::new(audit);
        app = app.layer(axum::middleware::from_fn(move |req, next| {
            let state = Arc::clone(&state);
            let audit = Arc::clone(&audit);
            async move { crate::audit::run(audit, state, req, next).await }
        }));
    }

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
