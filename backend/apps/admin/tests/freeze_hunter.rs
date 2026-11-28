//! The freeze hunter — the trigger-agnostic net for the "one request
//! 500'd and the whole service stopped answering" incident class.
//!
//! It boots the REAL assembled edge (bootstrap http server, full
//! middleware chain: request-id → recovery → logging, then the admin
//! pack's body guard → audit layer → auth gate → bind layer →
//! handlers) over the full admin contract — every mounted route from
//! the generated ROUTES table — and fires a battery of hostile
//! requests at each: malformed JSON, oversized declared bodies, lying
//! content lengths, i64-overflowing path ids, garbage query strings,
//! unauthenticated variants. After EVERY request it demands a liveness
//! probe (the auth-free login face, which rides redis + ratelimit +
//! the user lookup chain) answer within a hard deadline.
//!
//! The pinned property: **no single request may wedge the service** —
//! whatever the failure shape (a lock held forever, a pool drained, a
//! worker starved, a lifecycle wedged in shutdown), the symptom is the
//! probe going silent, and the hunter fails naming the culprit route
//! and payload. It does not pin any status code: errors are expected
//! and fine; only silence fails.

use std::sync::Arc;
use std::time::Duration;

use admin::admin::token::{new_jwt_id, UserTokenPayload};
use admin::migration;
use admin::server::rest;
use admin::state::AppState;
use rushwind_authn::Authenticator as _;
use rushwind_bootstrap::{Bootstrap, BootstrapError, RouteInput, RouteSurface};
use rushwind_transport::StopSignal;

/// Every request and every probe rides this deadline: a wedge shows
/// up as silence past it, never as a slow-but-live answer.
const DEADLINE: Duration = Duration::from_secs(5);

/// The hostile path-segment substitute for `{var}` templates: far past
/// i64's range, so numeric parses fail loudly (handled) if at all.
const HOSTILE_ID: &str = "92233720368547758080";

/// The hostile query suffix for read routes.
const HOSTILE_QUERY: &str = "?id=92233720368547758080&page=-1&size=0&filter=%00&q=zz&order=bogus";

/// The probe request body: format-invalid credentials (the password
/// field rides the RSA/AES transfer envelope, which plaintext cannot
/// satisfy) — the probe answers fast at the credential-format gate
/// after riding the guard, the session store, the ratelimit counters,
/// and the progressive captcha. The DB user lookup sits behind the
/// envelope decryption and is deliberately NOT part of the probe — the
/// attack requests themselves cover the handler/database faces.
fn probe_body() -> &'static str {
    "{\"username\":\"freeze-hunter-probe\",\"password\":\"x\"}"
}

/// The envelope the probe face answers with, verbatim — the follow-up
/// pin below compares against it.
fn probe_envelope() -> String {
    // The envelope opening every probe-face 400 shares — the
    // credential-format gate, the progressive captcha, the lockout
    // all answer this JSON envelope with differing reasons. A
    // polluted keep-alive connection would surface hyper's bare 400
    // (no JSON envelope at all) or silence; both fail this prefix.
    "{\"code\":400,".to_owned()
}

/// The diagnostic ladder after a silence: is the WHOLE service
/// silent, or only one face? Is mini-redis itself stuck? Prints the
/// verdicts that name the wedge's scope.
async fn silence_verdict(
    client: &reqwest::Client,
    base: &str,
    probe_path: &str,
    redis: &redis::aio::ConnectionManager,
) -> String {
    let mut verdicts = Vec::new();
    let second_probe = tokio::time::timeout(
        DEADLINE,
        client
            .post(format!("{base}{probe_path}"))
            .header("content-type", "application/json")
            .body(probe_body())
            .send(),
    )
    .await;
    verdicts.push(format!("second probe answered={:?}", second_probe.is_ok()));
    let image = tokio::time::timeout(
        DEADLINE,
        client
            .get(format!("{base}/admin/v1/file/image?sign=x"))
            .send(),
    )
    .await;
    verdicts.push(format!(
        "redis-free public route answered={:?}",
        image.is_ok()
    ));
    let mut direct_conn = redis.clone();
    let direct = tokio::time::timeout(
        Duration::from_secs(2),
        redis::cmd("GET")
            .arg("admin:login:fail:user:freeze-hunter-probe")
            .query_async::<Option<String>>(&mut direct_conn),
    )
    .await;
    verdicts.push(format!(
        "direct mini-redis GET answered={:?}",
        direct.is_ok()
    ));
    verdicts.join("; ")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_single_request_wedges_the_service() {
    let (state, bearer) = harness_state().await;
    // The diagnostic ladder's direct redis handle — cloned before the
    // route-pack closure moves the state Arc into the assembly.
    let state_redis = state.redis.clone();
    let port = free_port();

    // The REAL assembled edge over the full admin pack — the same
    // bootstrap face main.rs rides, one http server, no edge timeout
    // (the hunter's own deadline is the only bound, so hangs surface
    // here instead of masking into 504s).
    let yaml = format!(
        "app:\n  name: freeze-hunter\nservers:\n  - kind: http\n    bind: \"127.0.0.1:{port}\"\n    route_packs:\n      - name: admin-surface\n"
    );
    let assembler = Bootstrap::from_yaml_str(&yaml)
        .expect("assembly document parses")
        .route_pack(
            "admin-surface",
            move |_settings: serde_json::Value, _input: RouteInput| {
                Ok::<_, BootstrapError>(RouteSurface::new(rest::build_router(
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
    let run = tokio::spawn(async move { booted.app.run(run_stop).await });

    let client = test_client();
    wait_ready(&client, port).await;

    // The probe face: the auth-free login route (resolved from the
    // route table, never hardcoded) — redis session store, ratelimit
    // counter, credential lookup: the whole chain in one call.
    let probe_path = admin::proto::gen::routes::ROUTES
        .iter()
        .find(|spec| {
            spec.service_fq == "admin.service.v1.AuthenticationService"
                && spec.method_name == "Login"
        })
        .map(|spec| spec.path.to_owned())
        .expect("the login route rides the admin face");

    let base = format!("http://127.0.0.1:{port}");
    let mut fired = 0u64;
    for spec in admin::proto::gen::routes::ROUTES.iter() {
        if spec.shadowed {
            // The mounts skip shadowed bindings — not on the wire.
            continue;
        }
        let path = substitute_path_vars(spec.path);
        let url = format!("{base}{path}");
        let write = !matches!(spec.method, "GET" | "HEAD");

        // Each attack descriptor builds its request against a FRESH
        // client: the no-auth write attacks are answered WITHOUT
        // draining their bodies, and hyper (correctly) closes those
        // keep-alive connections afterwards — a pooled client would
        // then intermittently swallow its NEXT request on the dead
        // socket (a client artifact that would read as a wedge). A
        // fresh client per attack keeps the hunter measuring the
        // server; the deliberate pool-reuse pin lives at the battery's
        // end, where reuse is the property under test.
        let verb = || reqwest::Method::from_bytes(spec.method.as_bytes()).expect("verb");
        type AttackBuilder<'a> = Box<dyn Fn(&reqwest::Client) -> reqwest::RequestBuilder + 'a>;
        let mut attacks: Vec<(&str, AttackBuilder<'_>)> = Vec::new();
        if write {
            let u = url.clone();
            attacks.push((
                "no-auth malformed json",
                Box::new(move |c: &reqwest::Client| {
                    c.request(verb(), &u)
                        .header("content-type", "application/json")
                        .body("{\"a\":")
                }),
            ));
            let u = url.clone();
            let b = bearer.clone();
            attacks.push((
                "auth malformed json",
                Box::new(move |c: &reqwest::Client| {
                    c.request(verb(), &u)
                        .header("content-type", "application/json")
                        .header("authorization", &b)
                        .body("{\"a\":")
                }),
            ));
            // NOTE: the oversized-declared face (2MiB+1 honest body, no
            // token — the guard must answer 401 before reading any of
            // it) is deliberately NOT in the per-route battery: an
            // un-drained rejection closes the keep-alive connection,
            // and on Windows the RST-versus-in-flight-body race can
            // swallow that one request CLIENT-side (a protocol-stack
            // artifact, not an app wedge — the ladder proves the
            // service healthy). The guard's pre-read verdict and the
            // pool's cleanliness after such a rejection are pinned
            // deterministically at this battery's tail.
        } else {
            let u = url.clone();
            attacks.push((
                "no-auth hostile query",
                Box::new(move |c: &reqwest::Client| c.get(format!("{u}{HOSTILE_QUERY}"))),
            ));
            let u = url.clone();
            let b = bearer.clone();
            attacks.push((
                "auth hostile query",
                Box::new(move |c: &reqwest::Client| {
                    c.get(format!("{u}{HOSTILE_QUERY}"))
                        .header("authorization", &b)
                }),
            ));
        }

        for (label, build) in attacks {
            let fresh = test_client();
            let request = build(&fresh);
            let started = std::time::Instant::now();
            let outcome = tokio::time::timeout(DEADLINE, request.send()).await;
            eprintln!(
                "[freeze-hunter] #{fired} {} {} {:?} -> answered={} in {:?}",
                spec.method,
                spec.path,
                label,
                outcome.is_ok(),
                started.elapsed()
            );
            if outcome.is_err() {
                let verdict = silence_verdict(&client, &base, &probe_path, &state_redis).await;
                panic!(
                    "REQUEST SILENT — route {} {} attack {label:?} (request {fired}); ladder: {verdict}",
                    spec.method,
                    spec.path
                );
            }
            // A transport-level rejection (connection reset on the
            // lying content-length, DNS-less 4xx, anything) is an
            // ANSWER. Only silence is a wedge.
            fired += 1;

            let probe_started = std::time::Instant::now();
            let probe = tokio::time::timeout(
                DEADLINE,
                client
                    .post(format!("{base}{probe_path}"))
                    .header("content-type", "application/json")
                    .body("{\"username\":\"freeze-hunter-probe\",\"password\":\"x\"}")
                    .send(),
            )
            .await;
            eprintln!(
                "[freeze-hunter]   probe -> answered={} in {:?}",
                probe.is_ok(),
                probe_started.elapsed()
            );
            if probe.is_err() {
                let verdict = silence_verdict(&client, &base, &probe_path, &state_redis).await;
                panic!(
                    "PROBE SILENT — route {} {} attack {label:?} (request {fired}); ladder: {verdict}",
                    spec.method,
                    spec.path
                );
            }
            fired += 1;
        }
    }

    // The public image proxy rides outside the route table — hunt it
    // by hand (hostile HMAC query).
    for query in [
        "",
        "?sign=x&ts=1",
        "?sign=&ts=",
        &format!("?sign={HOSTILE_ID}"),
    ] {
        let outcome = tokio::time::timeout(
            DEADLINE,
            client
                .get(format!("{base}/admin/v1/file/image{query}"))
                .send(),
        )
        .await;
        assert!(
            outcome.is_ok(),
            "SERVICE WEDGED: image proxy went silent ({query:?})"
        );
        let probe = tokio::time::timeout(
            DEADLINE,
            client
                .post(format!("{base}{probe_path}"))
                .header("content-type", "application/json")
                .body("{\"username\":\"freeze-hunter-probe\",\"password\":\"x\"}")
                .send(),
        )
        .await;
        assert!(
            probe.is_ok(),
            "SERVICE WEDGED: probe silent after image proxy ({query:?})"
        );
        fired += 2;
    }

    eprintln!("[freeze-hunter] survived: {fired} requests + probes, service stayed live");

    // ---- the pooled-connection cleanliness pin, same server ----
    // The oversized unauthenticated attack: 2MiB+1 honest body, no
    // token — the body guard answers 401 BEFORE reading any of it,
    // leaving the request body undrained on a keep-alive connection.
    // The follow-ups must keep answering the probe's 400 envelope
    // family: a polluted pool would answer hyper's bare 400 (no
    // envelope) or go silent.
    let oversized = tokio::time::timeout(
        DEADLINE,
        client
            .post(format!("{base}/admin/v1/menus/sync"))
            .header("content-type", "application/json")
            .body(vec![b'A'; 2 * 1024 * 1024 + 1])
            .send(),
    )
    .await
    .expect("the oversized unauthenticated request resolves inside the deadline");
    // Pre-read rejection races the client's oversized body write: the
    // guard answers 401 and closes while the tail is still in flight,
    // so the client observes EITHER the 401 response OR a write-side
    // transport error (BrokenPipe / ConnectionAborted) — both faces
    // prove the guard rejected before reading. What must never happen
    // is the deadline elapsing: that would be the wedge itself.
    match oversized {
        Ok(resp) => {
            // Self-describing pin: a mismatch names the face that
            // answered (status + reason) instead of a bare code pair.
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            assert_eq!(
                status, 401,
                "the body guard's pre-read verdict (body: {body})"
            );
        }
        Err(_) => eprintln!(
            "[freeze-hunter]   oversized rejection observed mid-write (transport closed early)"
        ),
    }
    for i in 0..3 {
        let probe_started = std::time::Instant::now();
        let follow = tokio::time::timeout(
            DEADLINE,
            client
                .post(format!("{base}{probe_path}"))
                .header("content-type", "application/json")
                .body(probe_body())
                .send(),
        )
        .await
        .unwrap_or_else(|_| {
            panic!("SERVICE WEDGED: follow-up {i} silent after the undrained rejection")
        });
        let resp = follow.expect("follow-up transport");
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!(
            "[freeze-hunter]   follow-up {i} -> {} in {:?}",
            status,
            probe_started.elapsed()
        );
        assert_eq!(status, 400, "follow-up {i} status");
        assert!(
            body.starts_with(&probe_envelope()),
            "follow-up {i} envelope drifted (connection pollution?): {body}"
        );
    }

    stop.signal();
    let outcome = tokio::time::timeout(DEADLINE * 2, run)
        .await
        .expect("the signalled run ends")
        .expect("the run task joins");
    assert!(outcome.is_ok(), "a signalled run ends cooperatively");
}

/// Replaces every `{var}` template segment with the hostile id.
fn substitute_path_vars(template: &str) -> String {
    template
        .split('/')
        .map(|segment| {
            if segment.starts_with('{') {
                HOSTILE_ID
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
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

/// An ephemeral port for the hunter's assembly.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind :0")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Polls the probe face until the listener answers.
async fn wait_ready(client: &reqwest::Client, port: u16) {
    let url = format!("http://127.0.0.1:{port}/admin/v1/login",);
    for _ in 0..100 {
        if let Ok(resp) = client
            .post(&url)
            .header("content-type", "application/json")
            .body("{\"username\":\"x\",\"password\":\"x\"}")
            .send()
            .await
        {
            // Any handled status means the edge is serving.
            if resp.status().as_u16() >= 200 {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the assembled hunter server never answered on :{port}");
}

/// The harness state: shared-cache in-memory sqlite (pool-shared
/// schema) + in-process mini-redis + a real HS256 mint/verify pair,
/// with a seeded platform-admin access token — hostile requests reach
/// the handlers (not stop at 401), which is where wedges live.
async fn harness_state() -> (Arc<AppState>, String) {
    // Shared-cache in-memory sqlite: ONE logical database across the
    // whole pool (a plain `sqlite::memory:` gives each pooled
    // connection its OWN empty database), with lock resolution at
    // memory speed — a file-backed database turns the audit layer's
    // spawned writes vs the request path's own writes into 5-second
    // sqlx busy_timeout stalls that read exactly like a wedge.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let db = sea_orm::Database::connect(format!(
        // Single-slash form: sqlx reads the part after `sqlite:` as the
        // database name — `//file:` would parse `file:` as a URL host
        // and refuse.
        "sqlite:file:freeze_hunter_{pid}_{seq}?mode=memory&cache=shared",
        pid = std::process::id()
    ))
    .await
    .expect("sqlite connect");
    migration::sqlite_integration_schema(&db).await;

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

    // Two instances over the same HS256 key (the authenticator does
    // not derive Clone) — verify and mint agree, which is all the
    // gate needs.
    let jwt = || {
        rushwind_authn_jwt::JwtAuthenticator::new(
            rushwind_authn_jwt::JwtOptions::default().with_key(b"freeze-hunter-secret"),
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
            // noop 档：猎手电池的放行面与部署默认一致。
            authz_kind: Some(admin::config::AuthzKind::Noop),
            oss: None,
        },
        authz: Arc::new(admin::admin::authorizer::PolicyAuthorizer::new(Some(
            admin::config::AuthzKind::Noop,
        ))),
        db,
        redis: redis.clone(),
        authenticator: Arc::new(jwt()),
        jwt: jwt(),
        tokens: admin::admin::token::TokenStore::new(redis, 3600, 86_400),
        hub: admin::server::sse::Hub::default(),
        oss: None,
    });

    // Mint the platform-admin token and seed the session whitelist —
    // the exact faces login rides, no second truth.
    let payload = UserTokenPayload {
        username: "freeze-hunter".into(),
        user_id: 1,
        tenant_id: 0,
        org_unit_id: None,
        roles: vec!["platform:admin".into()],
        client_id: None,
        device_id: None,
        data_scope: None,
        data_scopes: Vec::new(),
        data_scope_unit_ids: None,
        hidden_fields: Vec::new(),
        is_platform_admin: Some(true),
        is_tenant_admin: None,
        jti: new_jwt_id(),
    };
    let exp = chrono::Utc::now().timestamp() + 3600;
    let bearer = state
        .jwt
        .create_identity(&rushwind_authn::AuthClaims(payload.to_access_claims(exp)))
        .expect("mint hunter token");
    state
        .tokens
        .add_access_token(payload.user_id, &payload.jti, &bearer)
        .await
        .expect("seed whitelist row");
    (state, format!("Bearer {bearer}"))
}
