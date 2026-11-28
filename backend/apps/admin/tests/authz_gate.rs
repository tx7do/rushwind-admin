//! The authz knob end to end: the REAL assembled edge over an
//! in-memory database, real minted tokens, and the policy snapshot
//! loaded from seeded rows — the same faces `main.rs` composes. Three
//! tests pin the knob's contract: `opa` permits exactly the bound
//! endpoint and 403s the unbound role, `noop` permits both (one trail
//! row each), and an absent stage (the section missing / `zanzibar`)
//! runs the route without any evaluation.

use std::sync::Arc;

use admin::admin::token::UserTokenPayload;
use admin::config::AuthzKind;
use admin::state::AppState;
use rushwind_authn::Authenticator as _;
use rushwind_bootstrap::{Bootstrap, BootstrapError, RouteInput, RouteSurface};
use rushwind_transport::StopSignal;
use sea_orm::{ActiveModelTrait as _, EntityTrait as _};

/// The gated list route the knob tests exercise (registered, unshadowed).
const ROUTE: &str = "/admin/v1/dict/langs";

/// The shared-cache in-memory database + the policy snapshot: role
/// `ops` holds a permission bound to [`ROUTE`] (GET); role `viewer`
/// holds nothing.
async fn harness_state(kind: Option<AuthzKind>) -> Arc<AppState> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let db = sea_orm::Database::connect(format!(
        "sqlite:file:authz_gate_{pid}_{seq}?mode=memory&cache=shared",
        pid = std::process::id()
    ))
    .await
    .expect("sqlite connect");
    admin::migration::sqlite_integration_schema(&db).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mini-redis bind");
    let addr = listener.local_addr().expect("mini-redis addr");
    tokio::spawn(async move {
        let _ = mini_redis::server::run(listener, std::future::pending::<()>()).await;
    });
    let client = redis::Client::open(format!("redis://{addr}/")).expect("redis client");
    let redis = client
        .get_connection_manager()
        .await
        .expect("redis connection manager");

    let jwt = || {
        rushwind_authn_jwt::JwtAuthenticator::new(
            rushwind_authn_jwt::JwtOptions::default().with_key(b"authz-gate-secret"),
        )
    };
    let state = Arc::new(AppState {
        cfg: admin::config::Config {
            database_source: String::new(),
            database_migrate: false,
            redis_addr: String::new(),
            redis_password: String::new(),
            jwt_private_key: None,
            jwt_public_key: None,
            access_token_expires_secs: 3600,
            refresh_token_expires_secs: 86_400,
            captcha_required_failures: 3,
            authz_kind: kind,
            oss: None,
        },
        authz: Arc::new(admin::admin::authorizer::PolicyAuthorizer::new(kind)),
        db: db.clone(),
        redis: redis.clone(),
        authenticator: Arc::new(jwt()),
        jwt: jwt(),
        tokens: admin::admin::token::TokenStore::new(redis, 3600, 86_400),
        hub: admin::server::sse::Hub::default(),
        oss: None,
    });

    let role = admin::data::sys_roles::ActiveModel {
        tenant_id: sea_orm::Set(Some(1)),
        name: sea_orm::Set("ops".into()),
        code: sea_orm::Set("ops".into()),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert role");
    let perm = admin::data::sys_permissions::ActiveModel {
        name: sea_orm::Set("dict langs read".into()),
        code: sea_orm::Set("dict:langs:view".into()),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert permission");
    admin::data::sys_role_permissions::ActiveModel {
        tenant_id: sea_orm::Set(Some(1)),
        role_id: sea_orm::Set(Some(role.id)),
        permission_id: sea_orm::Set(Some(perm.id)),
        effect: sea_orm::Set(Some("ALLOW".into())),
        status: sea_orm::Set(Some("ON".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert binding");
    admin::data::sys_permission_apis::ActiveModel {
        permission_id: sea_orm::Set(Some(perm.id)),
        endpoint_path: sea_orm::Set(Some(ROUTE.into())),
        endpoint_method: sea_orm::Set(Some("GET".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert endpoint");

    state.authz.reset(&db).await.expect("policy load");
    state
}

/// Mints a platform-scope token (tid 0 skips the tenant stage) holding
/// the one role, whitelisted exactly as login would.
async fn mint(state: &Arc<AppState>, role_code: &str, uid: i64) -> String {
    let payload = UserTokenPayload {
        username: format!("{role_code}-user"),
        user_id: uid,
        tenant_id: 0,
        org_unit_id: None,
        roles: vec![role_code.into()],
        client_id: None,
        device_id: None,
        data_scope: None,
        data_scopes: Vec::new(),
        data_scope_unit_ids: None,
        hidden_fields: Vec::new(),
        is_platform_admin: None,
        is_tenant_admin: None,
        jti: admin::admin::token::new_jwt_id(),
    };
    let exp = chrono::Utc::now().timestamp() + 3600;
    let bearer = state
        .jwt
        .create_identity(&rushwind_authn::AuthClaims(payload.to_access_claims(exp)))
        .expect("mint token");
    state
        .tokens
        .add_access_token(payload.user_id, &payload.jti, &bearer)
        .await
        .expect("seed session");
    format!("Bearer {bearer}")
}

/// The test client: `no_proxy` is not optional here — the host shells of
/// this project carry `HTTP_PROXY=127.0.0.1:7890` (the dev proxy), and
/// reqwest honors the env by default: localhost requests then ride the
/// proxy, which intermittently answers 502 instead of the edge (the
/// observed flake). Every client the tests build must bypass it.
fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind :0")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Boots the real assembled edge (the admin route pack over the whole
/// admin contract, docs off) on an ephemeral port.
async fn boot(state: Arc<AppState>) -> (String, StopSignal) {
    let port = free_port();
    let yaml = format!(
        "app:\n  name: authz-gate\nservers:\n  - kind: http\n    bind: \"127.0.0.1:{port}\"\n    route_packs:\n      - name: admin-surface\n"
    );
    let assembler = Bootstrap::from_yaml_str(&yaml)
        .expect("assembly document parses")
        .route_pack(
            "admin-surface",
            move |_settings: serde_json::Value, _input: RouteInput| {
                Ok::<_, BootstrapError>(RouteSurface::new(admin::server::rest::build_router(
                    Arc::clone(&state),
                    admin::server::docs::Wire {
                        enable_swagger: false,
                        enable_redoc: false,
                    },
                )))
            },
        );
    let booted = assembler.build().await.expect("assembly builds");
    let stop = StopSignal::new();
    let run_stop = stop.clone();
    tokio::spawn(async move { booted.app.run(run_stop).await });
    let client = test_client();
    for _ in 0..100 {
        if let Ok(resp) = client
            .post(format!("http://127.0.0.1:{port}/admin/v1/login"))
            .header("content-type", "application/json")
            .body("{\"username\":\"x\",\"password\":\"x\"}")
            .send()
            .await
        {
            if resp.status().as_u16() >= 200 {
                return (format!("http://127.0.0.1:{port}"), stop);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("edge never answered on :{port}");
}

async fn get(base: &str, bearer: &str) -> reqwest::Response {
    test_client()
        .get(format!("{base}{ROUTE}"))
        .header("authorization", bearer)
        .send()
        .await
        .expect("request")
}

async fn trail(state: &Arc<AppState>) -> Vec<admin::data::sys_policy_evaluation_logs::Model> {
    admin::data::sys_policy_evaluation_logs::Entity::find()
        .all(&state.db)
        .await
        .expect("trail rows")
}

/// opa 档：持有绑定端点的角色放行（200），未持有的角色 403
/// `unauthorized access`，每次尝试留痕。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn opa_knob_permits_the_bound_role_and_denies_the_rest() {
    let state = harness_state(Some(AuthzKind::Opa)).await;
    let (base, stop) = boot(Arc::clone(&state)).await;
    let ops = mint(&state, "ops", 11).await;
    let viewer = mint(&state, "viewer", 12).await;

    let ok = get(&base, &ops).await;
    assert_eq!(ok.status(), 200, "the bound endpoint permits");

    let denied = get(&base, &viewer).await;
    assert_eq!(denied.status(), 403, "the unbound role is denied");
    let body = denied.text().await.unwrap_or_default();
    assert!(
        body.contains("unauthorized access"),
        "the middleware's fixed denial text: {body}"
    );

    let rows = trail(&state).await;
    assert_eq!(rows.len(), 2, "one evaluation per attempt");
    assert!(rows.iter().all(|r| r
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .starts_with("engine=opa subject=")));
    assert_eq!(rows[0].result, Some(true));
    assert_eq!(rows[1].result, Some(false));

    stop.signal();
}

/// noop 档（部署默认）：未绑定端点的角色同样放行——旋钮的差异面。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn noop_knob_permits_the_unbound_role() {
    let state = harness_state(Some(AuthzKind::Noop)).await;
    let (base, stop) = boot(Arc::clone(&state)).await;
    let viewer = mint(&state, "viewer", 12).await;

    let resp = get(&base, &viewer).await;
    assert_eq!(resp.status(), 200);
    let rows = trail(&state).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0]
        .effect_details
        .as_deref()
        .unwrap_or_default()
        .starts_with("engine=noop subject="));
    stop.signal();
}

/// 引擎缺席（authz 节缺失 / zanzibar）：阶段不挂载——请求照过，
/// 轨迹零行。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn absent_knob_drops_the_stage() {
    let state = harness_state(None).await;
    let (base, stop) = boot(Arc::clone(&state)).await;
    let viewer = mint(&state, "viewer", 12).await;

    let resp = get(&base, &viewer).await;
    assert_eq!(resp.status(), 200);
    assert!(
        trail(&state).await.is_empty(),
        "the absent stage leaves no trail"
    );
    stop.signal();
}
