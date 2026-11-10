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
    let cfg = Config::load()?;
    // P3 hardening (the dev-key warning): the dev private key still
    // answering password envelopes means anyone holding this repo can
    // decrypt them — production must override
    // RUSHWIND_PASSWORD_RSA_PRIVATE_KEY.
    // (emitted raw: the log subsystem initializes inside the bootstrap)
    if admin::crypto::using_dev_password_private_key() {
        eprintln!(
            "⚠️  RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置，口令传输解密仍在使用内置开发私钥，生产环境必须替换"
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
    // external signal here stays unfired.
    let external = StopSignal::new();
    booted.app.run(external).await?;
    Ok(())
}
