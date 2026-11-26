//! The application assembly entry: loads the embedded config defaults
//! (env overrides win), connects Postgres + Redis, builds the JWT
//! engines (RS256 verify + mint), runs the migration chains (admin
//! first — the module seeds address its tables — then each business
//! module's own chain) and the idempotent seeds, and hands the
//! transports — the REST edge (BOTH route packs: the admin surface and
//! the business modules ride one http server, one edge, one audit
//! layer), the SSE notification server, the task-queue worker, the
//! cron producer — to the config-driven lifecycle assembler (the
//! embedded server document, one `App`, one shutdown path).
//!
//! Adding a business module is four lines in this file: the migration
//! chain, the seed, the route pack — and the `mod` declaration beside
//! the others. Everything else about a module lives in
//! `src/modules/<name>/`.

use std::sync::Arc;

use admin::config::Config;
use admin::modules::template;
use admin::server::{apalis, rest, sse};
use admin::state::AppState;
use rushwind_bootstrap::Bootstrap;
use rushwind_transport::StopSignal;

/// The server assembly document, compiled into the binary: the http
/// server carries BOTH route packs (the edge's CORS policy and the
/// request budget wrap them identically).
const SERVER_YAML: &str = include_str!("../assets/server.yaml");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The observability substrate, first of all: the framework layers
    // only EMIT tracing events — the per-request access lines
    // (rushwind.http: method/path/request_id/status/latency), the
    // panic reports (rushwind.panic), the lifecycle terminal
    // (rushwind.bootstrap) — and never install a subscriber. Without
    // this, every event lands in a no-op and the deployment is blind:
    // a failing dependency shows up as silent 10-second timeouts with
    // no line anywhere. `RUST_LOG` wins when set; the `info` default
    // keeps access lines, warns and errors visible everywhere (turn
    // statement/redis detail up per-target, e.g.
    // `RUST_LOG=info,sqlx=debug`).
    //
    // The writer is NON-BLOCKING on purpose: fmt's default writes
    // stdout synchronously, and a deployment that pipes stdout
    // somewhere nobody drains turns every log line into a worker
    // thread parked forever — the exact "process alive, everything
    // silent" jam. The appender's background thread owns the pipe;
    // emitters never block. ANSI is off so log files stay greppable.
    let (log_writer, log_guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(log_writer)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    // The worker guard must outlive every emit — main runs for the
    // process lifetime, so the binding here is the lifetime.
    let _log_guard = log_guard;

    let cfg = Config::load()?;
    // P3 hardening (the dev-key warning): the dev private key still
    // answering password envelopes means anyone holding this repo can
    // decrypt them — production must override
    // RUSHWIND_PASSWORD_RSA_PRIVATE_KEY.
    if admin::crypto::using_dev_password_private_key() {
        tracing::warn!(
            "RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置，口令传输解密仍在使用内置开发私钥，生产环境必须替换"
        );
    }
    let public_pem: &[u8] = match &cfg.jwt_public_key {
        Some(pem) => pem.as_bytes(),
        None => include_bytes!("../assets/jwt_public_key.pem"),
    };
    let verifier = rushwind_authn_jwt::JwtAuthenticator::new(
        rushwind_authn_jwt::JwtOptions::new()
            .with_algorithm("RS256")
            .map_err(|e| format!("jwt algorithm: {e:?}"))?
            .with_rsa_public_key_from_pem(public_pem)
            .map_err(|e| format!("jwt public key: {e:?}"))?,
    );
    let authenticator: Arc<dyn rushwind_authn::Authenticator> = Arc::new(verifier);

    let state = Arc::new(AppState::connect(cfg, Arc::clone(&authenticator)).await?);
    // The migration chains run in their own order: admin first (the
    // tables the module seeds address exist), then each module's own
    // chain. A new business module inserts its line here.
    if state.cfg.database_migrate {
        admin::migration::run(&state.db).await?;
        template::migration::run(&state.db).await?;
    }
    // The seeds run after ALL migrations; the module seeds depend on
    // the admin seeds' roles existing, so admin seeds go first.
    admin::seed::run(&state).await?;
    template::seed::run(&state).await?;

    // The shared task-queue storage handle: the worker (a factory
    // transport below) claims and executes; the periodic wildcard
    // pushes through. Business modules enqueue jobs on the same
    // queue instead of standing one up.
    let task_queue = Arc::new(apalis::TaskQueue::new(
        &state.cfg.database_source,
        "default",
    )?);

    let mut assembler = Bootstrap::from_yaml_str(SERVER_YAML)?
        .route_pack("admin-surface", rest::pack(Arc::clone(&state)))
        // The business surface rides the same http server and edge —
        // one more line per module pack.
        .route_pack("business-surface", template::pack(Arc::clone(&state)));
    for (name, job) in apalis::cron_jobs(Arc::clone(&state), Arc::clone(&task_queue)) {
        assembler = assembler.cron_job(name, job);
    }
    let assembler = assembler
        .server_factory("admin-sse", sse::factory(Arc::clone(&state)))
        .server_factory(
            "admin-tasks",
            apalis::worker_factory(Arc::clone(&state), Arc::clone(&task_queue)),
        );

    let booted = assembler.build().await?;
    // The lifecycle owns the OS-signal shutdown path internally; the
    // external signal here stays unfired. run_logged keeps the
    // terminal outcome visible: a non-cooperative end (panicked
    // server, teardown timeout, failed bind) lands one error line
    // under rushwind.bootstrap, while raise-time panics already reach
    // rushwind.panic through the hook the assembly's panic_reporting
    // toggle plugged.
    let external = StopSignal::new();
    booted.run_logged(external).await?;
    Ok(())
}
