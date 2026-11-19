//! 测试面装配：sqlite 内存库 + 进程内 mini-redis 的最小 [`AppState`]。
//! 仅测试构建（lib.rs 的 `#[cfg(test)]` 门）——prod 装配走
//! `AppState::connect`（真实 Postgres/Redis/RS256），本模块只为
//! `#[tokio::test]` 提供可直接调用 service handler 的最小状态：
//!
//! * `db`：`sqlite::memory:`，schema 由 [`SqliteTestMigrator`]（与 prod
//!   同一实体清单，backend=Sqlite）经 sea-orm-migration 追踪表建全；
//! * `redis`：绑定 127.0.0.1:0 的进程内 mini-redis——纯 DB service
//!   从不触它，但 `AppState.redis` 的 `ConnectionManager` 必须有一个
//!   活连接才能构造；
//! * `jwt`/`authenticator`：纯 DB 面不校验 token，`JwtOptions::default()`
//!   （HS256 无密钥）与 [`FakeAuthenticator`] 占位即可。
//!
//! CI 零服务依赖：`cargo test --workspace` 不需要 postgres/redis 在场。

use std::sync::Arc;

use rushwind_authn::{AuthClaims, Authenticator, AuthnError, SCHEME_BEARER};
use rushwind_http_binding::ctx::RequestContext;
use rushwind_storage_seaorm_migration::MigratorTrait as _;
use sea_orm::{Database, DatabaseConnection};

use crate::migration::SqliteTestMigrator;
use crate::state::AppState;

/// 全新内存库（每次连接即独立库，测试间天然隔离）。
pub(crate) async fn mem_db() -> DatabaseConnection {
    Database::connect("sqlite::memory:")
        .await
        .expect("sqlite::memory: connect")
}

/// 建全部实体表（与 prod 同一迁移清单，backend=Sqlite）。
pub(crate) async fn schema(db: &DatabaseConnection) {
    SqliteTestMigrator::up(db, None)
        .await
        .expect("sqlite schema up");
}

/// 进程内 mini-redis 上的连接管理器（纯 DB service 不用它，仅撑起
/// `AppState.redis` 的构造）。
async fn redis_conn() -> redis::aio::ConnectionManager {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mini-redis bind");
    let addr = listener.local_addr().expect("mini-redis addr");
    // shutdown 用永不完成的 pending——测试进程退出时后台任务随之消亡。
    tokio::spawn(async move {
        let _ = mini_redis::server::run(listener, std::future::pending::<()>()).await;
    });
    let client = redis::Client::open(format!("redis://{addr}/")).expect("redis client");
    client
        .get_connection_manager()
        .await
        .expect("redis connection manager")
}

/// 最小可用的 `AppState`（内存库 + 进程内 redis + 占位认证面）。
pub(crate) async fn test_state() -> Arc<AppState> {
    let db = mem_db().await;
    schema(&db).await;
    let redis = redis_conn().await;
    let cfg = crate::config::Config {
        database_source: String::new(),
        database_migrate: false,
        redis_addr: String::new(),
        redis_password: String::new(),
        jwt_private_key: None,
        jwt_public_key: None,
        access_token_expires_secs: 3600,
        refresh_token_expires_secs: 86_400,
        captcha_required_failures: 3,
        oss: None,
    };
    Arc::new(AppState {
        cfg,
        db,
        redis: redis.clone(),
        authenticator: Arc::new(FakeAuthenticator),
        // HS256 测试钥：默认无钥面铸造必败——渐进验证码等登录链路测试
        // 要走通 token 铸造（既有测试从不铸造，不受影响）。
        jwt: rushwind_authn_jwt::JwtAuthenticator::new(
            rushwind_authn_jwt::JwtOptions::default().with_key(b"admin-api-test-secret"),
        ),
        tokens: crate::admin::token::TokenStore::new(redis, 3600, 86_400),
        hub: crate::server::sse::Hub::default(),
        oss: None,
    })
}

/// 占位认证面：测试面从不校验/铸造 token，两个引擎方向都拒绝即可。
pub(crate) struct FakeAuthenticator;

impl Authenticator for FakeAuthenticator {
    fn scheme(&self) -> &'static str {
        SCHEME_BEARER
    }

    fn authenticate_token(&self, _token: &str) -> Result<AuthClaims, AuthnError> {
        Err(AuthnError::InvalidToken)
    }

    fn create_identity(&self, _claims: &AuthClaims) -> Result<String, AuthnError> {
        Err(AuthnError::InvalidToken)
    }
}

/// 带 `sub`/`uid`/`tid` 声明包的请求上下文（键名对齐
/// `UserTokenPayload::from_claims`；`tid = 0` 即平台视角）。
pub(crate) fn ctx(tenant_id: i64, user_id: i64) -> RequestContext {
    let mut claims = serde_json::Map::new();
    claims.insert(
        "sub".into(),
        serde_json::Value::String(format!("user{user_id}")),
    );
    claims.insert("uid".into(), serde_json::json!(user_id));
    claims.insert("tid".into(), serde_json::json!(tenant_id));
    RequestContext {
        claims: Some(claims),
        ..Default::default()
    }
}
