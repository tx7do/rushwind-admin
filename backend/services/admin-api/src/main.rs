//! The admin service assembly entry: loads the embedded config
//! defaults (env overrides win), connects Postgres + Redis, builds
//! the JWT engines (RS256 verify + mint), wires the service layer,
//! and hands the transports — the REST edge, the SSE notification
//! server, the task-queue worker, the cron producer — to the
//! config-driven lifecycle assembler (the embedded server document,
//! one `App`, one shutdown path). Everything lives in the library
//! crate; this binary only composes it.

use std::sync::Arc;

use admin_api::config::Config;
use admin_api::migration;
use admin_api::seed;
use admin_api::server::{apalis, rest, sse};
use admin_api::state::AppState;
use rushwind_bootstrap::Bootstrap;
use rushwind_transport::StopSignal;

/// The server assembly document, compiled into the binary.
const SERVER_YAML: &str = include_str!("../assets/server.yaml");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::load()?;
    // P3 hardening: the dev private key still answering password
    // envelopes means anyone holding this repo can decrypt them —
    // production must override RUSHWIND_PASSWORD_RSA_PRIVATE_KEY.
    // (emitted raw: the log subsystem initializes inside the bootstrap)
    if admin_api::crypto::using_dev_password_private_key() {
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
    if state.cfg.database_migrate {
        migration::run(&state.db).await?;
    }
    seed::run(&state).await?;

    // The shared task-queue storage handle: the worker (a factory
    // transport below) claims and executes; the periodic wildcard
    // pushes through.
    let task_queue = Arc::new(apalis::TaskQueue::new(
        &state.cfg.database_source,
        "default",
    )?);

    let mut assembler = Bootstrap::from_yaml_str(SERVER_YAML)?
        .route_pack("admin-surface", rest::pack(Arc::clone(&state)));
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
