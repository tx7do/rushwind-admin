//! The fork's assembly entry: the admin surface AND the business
//! module surface in one process — one auth gate, one audit layer,
//! one lifecycle. The assembly order is the admin binary's plus the
//! module's two insertions: the module migration chain right after
//! the admin's, and the business route pack beside the admin's.
//!
//! This directory is the fork's to edit freely; upgrading upstream
//! touches only the dependency revisions, not this file.

use std::sync::Arc;

use admin_api::config::Config;
use admin_api::server::{apalis, rest, sse};
use admin_api::state::AppState;
use rushwind_bootstrap::Bootstrap;
use rushwind_transport::StopSignal;

/// The server assembly document, compiled into the binary: the http
/// server carries BOTH route packs (the edge's CORS policy and the
/// request budget wrap them identically).
const SERVER_YAML: &str = include_str!("../assets/server.yaml");

/// The dev JWT public key, copied from the admin assets — the demo
/// keypair verifies the demo credentials; production overrides
/// `jwt.public_key` (config) with the real key.
const DEV_JWT_PUBLIC_KEY: &[u8] = include_bytes!("../assets/jwt_public_key.pem");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::load()?;
    // P3 hardening (mirrors the admin binary): the dev private key
    // still answering password envelopes means anyone holding this
    // repo can decrypt them — production must override
    // RUSHWIND_PASSWORD_RSA_PRIVATE_KEY.
    if admin_api::crypto::using_dev_password_private_key() {
        eprintln!(
            "⚠️  RUSHWIND_PASSWORD_RSA_PRIVATE_KEY 未配置，口令传输解密仍在使用内置开发私钥，生产环境必须替换"
        );
    }
    let public_pem: &[u8] = match &cfg.jwt_public_key {
        Some(pem) => pem.as_bytes(),
        None => DEV_JWT_PUBLIC_KEY,
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
        admin_api::migration::run(&state.db).await?;
        module_template::migration::run(&state.db).await?;
    }
    // The seeds run after ALL migrations; the module seeds depend on
    // the admin seeds' roles existing, so admin seeds go first.
    admin_api::seed::run(&state).await?;
    module_template::seed::run(&state).await?;

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
        .route_pack(
            "business-surface",
            module_template::pack(Arc::clone(&state)),
        );
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
