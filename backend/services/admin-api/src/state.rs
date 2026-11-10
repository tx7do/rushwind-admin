//! The shared application state: config, database pool, Redis, the JWT
//! engines (verify + mint) and the token/session store.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use crate::config::Config;
use crate::token::{TokenStore, UserTokenPayload};

pub struct AppState {
    pub cfg: Config,
    pub db: DatabaseConnection,
    pub redis: ConnectionManager,
    /// The verification engine the auth gate rides (public key).
    pub authenticator: Arc<dyn rushwind_authn::Authenticator>,
    /// The mint engine (private key) — RS256.
    pub jwt: rushwind_authn_jwt::JwtAuthenticator,
    pub tokens: TokenStore,
    /// The SSE notification hub (`/events` subscribers).
    pub hub: crate::server::sse::Hub,
    /// The object-storage engines, one per content bucket, present when
    /// `oss.yaml` carries a MinIO section.
    pub oss: Option<OssBuckets>,
}

/// The five content buckets the file service routes objects into, all
/// on the same MinIO endpoint.
pub struct OssBuckets {
    pub buckets: std::collections::HashMap<&'static str, Arc<dyn rushwind_oss::ObjectStorage>>,
    pub upload_host: String,
    pub download_host: String,
}

impl OssBuckets {
    pub const NAMES: [&'static str; 5] = ["images", "videos", "audios", "docs", "files"];

    /// The engine for a MIME-derived bucket name.
    pub fn storage(&self, bucket: &str) -> Option<Arc<dyn rushwind_oss::ObjectStorage>> {
        self.buckets.get(bucket).cloned()
    }
}

impl AppState {
    /// Connects Postgres + Redis and builds the JWT engines. `migrate`
    /// wiring (schema bootstrap) is owned by the golden-DDL pipeline; the
    /// database already carries the schema via its own migration.
    pub async fn connect(
        cfg: Config,
        authenticator: Arc<dyn rushwind_authn::Authenticator>,
    ) -> Result<Self, String> {
        let mut opts = ConnectOptions::new(cfg.database_source.clone());
        opts.max_connections(25)
            .min_connections(25)
            .connect_timeout(std::time::Duration::from_secs(10));
        let db = Database::connect(opts)
            .await
            .map_err(|e| format!("postgres connect: {e}"))?;

        let client = redis::Client::open(format!(
            "redis://:{}@{}/",
            cfg.redis_password, cfg.redis_addr
        ))
        .map_err(|e| format!("redis url: {e}"))?;
        let redis_conn = client
            .get_connection_manager()
            .await
            .map_err(|e| format!("redis connect: {e}"))?;

        // The mint engine — auth.yaml private key (or env).
        let private_pem = cfg
            .jwt_private_key
            .clone()
            .ok_or("auth.yaml private_key missing (RS256 minting requires it)")?;
        let jwt = rushwind_authn_jwt::JwtAuthenticator::new(
            rushwind_authn_jwt::JwtOptions::new()
                .with_algorithm("RS256")
                .map_err(|e| format!("jwt algorithm: {e:?}"))?
                .with_rsa_private_key_from_pem(private_pem.as_bytes())
                .map_err(|e| format!("jwt private key: {e:?}"))?,
        );

        let tokens = TokenStore::new(
            redis_conn.clone(),
            cfg.access_token_expires_secs,
            cfg.refresh_token_expires_secs,
        );

        // The object-storage engines: one per content bucket over the
        // MinIO section of oss.yaml (path-style addressing — the MinIO
        // requirement). Bucket creation is best-effort here; an
        // already-owned bucket answers conflict and is fine.
        let oss = match cfg.oss.as_ref() {
            Some(oss) => {
                let mut buckets = std::collections::HashMap::new();
                for name in OssBuckets::NAMES {
                    let engine = rushwind_oss_s3::S3Storage::new(rushwind_oss::StorageConfig {
                        endpoint: oss.endpoint.clone(),
                        region: "us-east-1".to_string(),
                        access_key: oss.access_key.clone(),
                        secret_key: oss.secret_key.clone(),
                        token: None,
                        use_ssl: oss.use_ssl,
                        force_path_style: true,
                        bucket: name.to_string(),
                    })
                    .map_err(|e| format!("oss storage {name}: {e}"))?;
                    // Idempotent bootstrap: an already-owned bucket
                    // answers conflict — any failure here defers to
                    // the first put.
                    let _ = engine.create_bucket().await;
                    buckets.insert(
                        name,
                        Arc::new(engine) as Arc<dyn rushwind_oss::ObjectStorage>,
                    );
                }
                Some(OssBuckets {
                    buckets,
                    upload_host: oss.upload_host.clone(),
                    download_host: oss.download_host.clone(),
                })
            }
            None => None,
        };

        Ok(Self {
            cfg,
            db,
            redis: redis_conn,
            authenticator,
            jwt,
            tokens,
            hub: crate::server::sse::Hub::default(),
            oss,
        })
    }
}

/// The error envelope helper: reason → the admin error table's HTTP
/// status (`StatusError::new`) and the `<Reason>` literal.
pub fn status_error(
    reason: &'static str,
    message: impl Into<String>,
) -> rushwind_http_binding::envelope::StatusError {
    let status = proto::tables::error_status("admin.service.v1", reason).unwrap_or(500);
    rushwind_http_binding::envelope::StatusError::new(status, reason, message)
}

/// The Unknown branch: 500 + empty reason.
pub fn internal_error(message: impl Into<String>) -> rushwind_http_binding::envelope::StatusError {
    rushwind_http_binding::envelope::StatusError::new(500, "", message)
}

/// The missing-identity error for service-side operator extraction.
pub fn operator_missing() -> StatusError {
    status_error("UNAUTHORIZED", "missing identity")
}

pub type StatusError = rushwind_http_binding::envelope::StatusError;

/// The verified operator extracted from the request context claims.
pub fn operator_of(
    ctx: &rushwind_http_binding::ctx::RequestContext,
) -> Result<UserTokenPayload, StatusError> {
    ctx.claims
        .as_ref()
        .and_then(UserTokenPayload::from_claims)
        .ok_or_else(|| status_error("UNAUTHORIZED", "missing identity"))
}

/// The operator's tenant, platform (0) when no identity rides.
pub fn tenant_of(ctx: &rushwind_http_binding::ctx::RequestContext) -> i64 {
    operator_of(ctx).map(|p| p.tenant_id).unwrap_or(0)
}

// The repository-layer DB failure mapping lives in the framework
// bridge crate; re-exported so the repositories' paths stay put.
pub use rushwind_storage_seaorm_support::db_err;

pub fn not_found(what: &str) -> StatusError {
    status_error("NOT_FOUND", format!("{what} not found"))
}

/// The DB status literal → the wire status code ("OFF" is the only
/// non-active spelling the schema carries).
pub fn status_to_proto(s: &str) -> i32 {
    if s == "OFF" {
        0
    } else {
        1
    }
}

/// The required-payload extraction for create-style requests.
pub fn require_data<T>(data: Option<T>) -> Result<T, StatusError> {
    data.ok_or_else(|| status_error("BAD_REQUEST", "data required"))
}

// The entity-time ↔ proto-Timestamp conversions live in the framework
// bridge crate; re-exported so the mappers' paths stay put (the
// roundtrip is pinned by the crate's own suite).
pub use rushwind_storage_seaorm_support::time::{naive_to_ts, ts_to_naive};
