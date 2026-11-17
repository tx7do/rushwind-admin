//! Config loading — parses the embedded yaml defaults
//! (`assets/data.yaml`, `assets/auth.yaml`, `assets/oss.yaml`,
//! compiled into the binary) with the same env overrides
//! (`RUSHWIND_AUTH_JWT_*`,
//! plus `RUSHWIND_DATABASE_SOURCE` / `RUSHWIND_REDIS_ADDR` / `RUSHWIND_REDIS_PASSWORD`
//! for out-of-container runs). The server assembly document
//! (`assets/server.yaml`) belongs to the lifecycle assembler.

use rushwind_bootstrap::DurationWire;
use serde::Deserialize;

const DATA_YAML: &str = include_str!("../assets/data.yaml");
const AUTH_YAML: &str = include_str!("../assets/auth.yaml");
const OSS_YAML: &str = include_str!("../assets/oss.yaml");

#[derive(Debug, Clone)]
pub struct Config {
    pub database_source: String,
    /// Startup gate: run pending schema migrations before seeding.
    pub database_migrate: bool,
    pub redis_addr: String,
    pub redis_password: String,
    /// RS256 private key PEM (minting) — from auth.yaml or the env override.
    pub jwt_private_key: Option<String>,
    /// RS256 public key PEM (verification) — falls back to the vendored
    /// jwt_public_key.pem when auth.yaml carries none.
    pub jwt_public_key: Option<String>,
    pub access_token_expires_secs: i64,
    pub refresh_token_expires_secs: i64,
    /// 渐进式验证码阈值（issue #1）：同一 (client_ip, username) 登录失败
    /// 达此次数后，后续登录要求验证码（缺失/错误即 CAPTCHA_REQUIRED /
    /// CAPTCHA_INVALID）。默认 3，锁定阈值（5）之下。
    pub captcha_required_failures: i64,
    #[allow(dead_code)]
    pub oss: Option<OssConfig>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // object-storage section, wired with the file services
pub struct OssConfig {
    pub endpoint: String,
    pub upload_host: String,
    pub download_host: String,
    pub access_key: String,
    pub secret_key: String,
    pub use_ssl: bool,
}

#[derive(Debug, Deserialize)]
struct DataFile {
    data: Option<DataSection>,
}

#[derive(Debug, Default, Deserialize)]
struct DataSection {
    database: Option<DatabaseSection>,
    redis: Option<RedisSection>,
}

#[derive(Debug, Default, Deserialize)]
struct DatabaseSection {
    #[serde(default)]
    #[allow(dead_code)] // auth.yaml parity; the yaml carries the connection
    driver: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    migrate: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RedisSection {
    #[serde(default)]
    addr: String,
    #[serde(default)]
    password: String,
}

#[derive(Debug, Deserialize)]
struct AuthFile {
    authn: Option<AuthnSection>,
}

#[derive(Debug, Deserialize)]
struct AuthnSection {
    jwt: Option<JwtSection>,
}

#[derive(Debug, Deserialize)]
struct JwtSection {
    #[serde(default)]
    #[allow(dead_code)] // taken verbatim; RS256 params come from the dedicated keys
    method: String,
    #[serde(default)]
    #[allow(dead_code)]
    key: String,
    #[serde(default)]
    private_key: String,
    #[serde(default)]
    public_key: String,
    #[serde(default)]
    access_token_expires: Option<DurationWire>,
    #[serde(default)]
    refresh_token_expires: Option<DurationWire>,
}

#[derive(Debug, Deserialize)]
struct OssFile {
    oss: Option<OssSection>,
}

#[derive(Debug, Deserialize)]
struct OssSection {
    #[serde(default)]
    minio: Option<MinioSection>,
}

#[derive(Debug, Default, Deserialize)]
struct MinioSection {
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    upload_host: String,
    #[serde(default)]
    download_host: String,
    #[serde(default)]
    access_key: String,
    #[serde(default)]
    secret_key: String,
    #[serde(default)]
    use_ssl: bool,
}

impl Config {
    /// Loads the vendored yaml files with env overrides applied.
    pub fn load() -> Result<Self, String> {
        let data: DataFile =
            serde_yaml::from_str(DATA_YAML).map_err(|e| format!("parse data.yaml: {e}"))?;
        let auth: AuthFile =
            serde_yaml::from_str(AUTH_YAML).map_err(|e| format!("parse auth.yaml: {e}"))?;
        let oss: OssFile = serde_yaml::from_str(OSS_YAML).unwrap_or(OssFile { oss: None });

        let data_section = data.data.unwrap_or_default();
        let database = data_section.database.unwrap_or_default();
        let redis_section = data_section.redis.unwrap_or_default();
        let jwt = auth.authn.and_then(|a| a.jwt).unwrap_or(JwtSection {
            method: "HS256".into(),
            key: String::new(),
            private_key: String::new(),
            public_key: String::new(),
            access_token_expires: None,
            refresh_token_expires: None,
        });

        // The code defaults : access 15 min,
        // refresh 7 days.
        let access_token_expires_secs = env_int(
            "RUSHWIND_ACCESS_TOKEN_EXPIRES_SECS",
            jwt.access_token_expires
                .as_ref()
                .map(|d| d.0.as_secs() as i64),
            900,
        );
        let captcha_required_failures =
            env_int("RUSHWIND_CAPTCHA_REQUIRED_FAILURES", None, 3).clamp(1, 4);
        let refresh_token_expires_secs = env_int(
            "RUSHWIND_REFRESH_TOKEN_EXPIRES_SECS",
            jwt.refresh_token_expires
                .as_ref()
                .map(|d| d.0.as_secs() as i64),
            7 * 24 * 3600,
        );

        let mut minio = oss.oss.and_then(|o| o.minio).unwrap_or_default();
        // Host-side runs reach the published port, not the network alias.
        if let Some(host_endpoint) = env_non_empty("RUSHWIND_OSS_ENDPOINT") {
            minio.endpoint = host_endpoint;
        }

        Ok(Config {
            database_source: env_string("RUSHWIND_DATABASE_SOURCE", database.source),
            database_migrate: env_bool("RUSHWIND_DATABASE_MIGRATE", database.migrate),
            redis_addr: env_string("RUSHWIND_REDIS_ADDR", redis_section.addr),
            redis_password: env_string("RUSHWIND_REDIS_PASSWORD", redis_section.password),
            jwt_private_key: env_non_empty_or(
                "RUSHWIND_AUTH_JWT_PRIVATE_KEY",
                non_empty(jwt.private_key.clone()),
            ),
            jwt_public_key: env_non_empty_or(
                "RUSHWIND_AUTH_JWT_PUBLIC_KEY",
                non_empty(jwt.public_key.clone()),
            ),
            access_token_expires_secs,
            captcha_required_failures,
            refresh_token_expires_secs,
            oss: (!minio.endpoint.is_empty()).then_some(OssConfig {
                endpoint: minio.endpoint,
                upload_host: minio.upload_host,
                download_host: minio.download_host,
                access_key: minio.access_key,
                secret_key: minio.secret_key,
                use_ssl: minio.use_ssl,
            }),
        })
    }
}

/// The env-override forms the loader uses. Each returns the yaml (or
/// code) default unless the environment carries a value of the expected
/// shape.
fn env_int(name: &str, yaml: Option<i64>, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .or(yaml)
        .unwrap_or(default)
}

fn env_string(name: &str, yaml: String) -> String {
    std::env::var(name).unwrap_or(yaml)
}

fn env_bool(name: &str, yaml: bool) -> bool {
    std::env::var(name)
        .map(|v| v == "true" || v == "1")
        .unwrap_or(yaml)
}

/// A set, non-empty environment value — `None` when unset or empty.
fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// A set, non-empty environment value, else a non-empty yaml value.
fn env_non_empty_or(name: &str, yaml: Option<String>) -> Option<String> {
    env_non_empty(name).or(yaml)
}

fn non_empty(v: String) -> Option<String> {
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::{env_bool, env_int, env_non_empty, env_non_empty_or, env_string};
    use std::env::{remove_var, set_var};

    // Unique names so parallel tests never collide on the shared
    // environment.
    const INT: &str = "RUSHWIND_TEST_ENV_HELPER_INT";
    const STRING: &str = "RUSHWIND_TEST_ENV_HELPER_STRING";
    const BOOL: &str = "RUSHWIND_TEST_ENV_HELPER_BOOL";
    const NON_EMPTY: &str = "RUSHWIND_TEST_ENV_HELPER_NON_EMPTY";

    #[test]
    fn env_int_takes_parseable_override_else_yaml_else_default() {
        set_var(INT, "1234");
        assert_eq!(env_int(INT, None, 42), 1234);
        set_var(INT, "not-a-number");
        assert_eq!(env_int(INT, Some(7), 42), 7);
        remove_var(INT);
        assert_eq!(env_int(INT, None, 42), 42);
    }

    #[test]
    fn env_string_takes_set_value_verbatim_else_yaml() {
        set_var(STRING, "");
        assert_eq!(env_string(STRING, "yaml".into()), "");
        remove_var(STRING);
        assert_eq!(env_string(STRING, "yaml".into()), "yaml");
    }

    #[test]
    fn env_bool_takes_true_or_1_else_yaml() {
        set_var(BOOL, "true");
        assert!(env_bool(BOOL, false));
        set_var(BOOL, "1");
        assert!(env_bool(BOOL, false));
        set_var(BOOL, "yes");
        assert!(!env_bool(BOOL, false));
        remove_var(BOOL);
        assert!(!env_bool(BOOL, false));
        assert!(env_bool(BOOL, true));
    }

    #[test]
    fn env_non_empty_skips_empty_values() {
        set_var(NON_EMPTY, "");
        assert_eq!(env_non_empty(NON_EMPTY), None);
        set_var(NON_EMPTY, "value");
        assert_eq!(env_non_empty(NON_EMPTY), Some("value".to_string()));
        remove_var(NON_EMPTY);
        assert_eq!(env_non_empty(NON_EMPTY), None);
        assert_eq!(
            env_non_empty_or(NON_EMPTY, Some("fallback".into())),
            Some("fallback".to_string())
        );
    }
}
