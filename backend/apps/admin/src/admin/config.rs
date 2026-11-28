//! Config loading — parses the runtime yaml files (`configs/data.yaml`,
//! `configs/auth.yaml`, `configs/oss.yaml`) with the same env overrides
//! (`RUSHWIND_AUTH_JWT_*`,
//! plus `RUSHWIND_DATABASE_SOURCE` / `RUSHWIND_REDIS_ADDR` / `RUSHWIND_REDIS_PASSWORD`
//! for out-of-container runs). The server assembly document
//! (`configs/server.yaml`) belongs to the lifecycle assembler; the
//! OpenAPI spec (`configs/openapi.yaml`) to the docs/SyncApis face.
//!
//! The yaml files are NOT embedded: every one is read from disk at
//! startup, so operators edit configs in place and restart. The
//! directory resolves through [`config_dir`].

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rushwind_bootstrap::DurationWire;
use serde::Deserialize;

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
    /// The authz engine selector (auth.yaml `authz.type`) — the gate's
    /// authorization-stage choice. `None` = the stage is ABSENT (no
    /// evaluation, no trail): the section missing entirely, or
    /// `zanzibar` (the reference's unimplemented engine answers nil,
    /// which drops the middleware).
    pub authz_kind: Option<AuthzKind>,
    #[allow(dead_code)]
    pub oss: Option<OssConfig>,
}

/// The authz engine kind (`authz.type`): `noop` permits every subject
/// (the default deployment), `casbin`/`opa` evaluate the loaded
/// role→endpoint policy snapshot. Unknown spellings fall to `Noop` —
/// the reference's switch default arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzKind {
    Noop,
    Casbin,
    Opa,
}

impl AuthzKind {
    /// The engine name the audit trail's `engine=` face and the
    /// evaluation context carry.
    pub fn engine_name(self) -> &'static str {
        match self {
            AuthzKind::Noop => "noop",
            AuthzKind::Casbin => "casbin",
            AuthzKind::Opa => "opa",
        }
    }
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
    authz: Option<AuthzSection>,
}

/// auth.yaml `authz:` — only `type` decides; the casbin/opa/zanzibar
/// sub-sections are config-face parity (serde drops the unknown keys).
#[derive(Debug, Default, Deserialize)]
struct AuthzSection {
    #[serde(default, rename = "type")]
    kind: String,
}

/// The section → engine mapping, the reference's switch verbatim: the
/// section missing → stage absent (cfg nil, engine nil, middleware
/// dropped); `""`/`noop`/unknown → noop (the default arm); casbin/opa →
/// their engines; `zanzibar` → the unimplemented engine answers nil →
/// stage absent.
fn authz_kind_of(section: Option<AuthzSection>) -> Option<AuthzKind> {
    match section?.kind.as_str() {
        "casbin" => Some(AuthzKind::Casbin),
        "opa" => Some(AuthzKind::Opa),
        "zanzibar" => None,
        _ => Some(AuthzKind::Noop),
    }
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
    /// Loads the runtime yaml files with env overrides applied.
    pub fn load() -> Result<Self, String> {
        let data: DataFile = serde_yaml::from_str(&read_config_file("data.yaml")?)
            .map_err(|e| format!("parse data.yaml: {e}"))?;
        let auth: AuthFile = serde_yaml::from_str(&read_config_file("auth.yaml")?)
            .map_err(|e| format!("parse auth.yaml: {e}"))?;
        // Optional: a deployment without object storage simply omits the
        // file; a present-but-malformed file is an error (fail-closed,
        // no silent storage loss).
        let oss: OssFile = match read_optional_config_file("oss.yaml") {
            Some(text) => {
                serde_yaml::from_str(&text).map_err(|e| format!("parse oss.yaml: {e}"))?
            }
            None => OssFile { oss: None },
        };

        let data_section = data.data.unwrap_or_default();
        let database = data_section.database.unwrap_or_default();
        let redis_section = data_section.redis.unwrap_or_default();
        // The authz engine knob: auth.yaml by default, the env override
        // for deployments that flip engines without editing the file
        // (same spelling rules — empty/unknown → noop, zanzibar → the
        // stage absent).
        let authz_kind = match env_non_empty("RUSHWIND_AUTHZ_TYPE") {
            Some(kind) => authz_kind_of(Some(AuthzSection { kind })),
            None => authz_kind_of(auth.authz),
        };
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
            authz_kind,
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

// ─────────────────────── the runtime config directory ───────────────────────

/// The configuration directory: every runtime document (`data.yaml`,
/// `auth.yaml`, `server.yaml`, the optional `oss.yaml` /
/// `jwt_public_key.pem`) lives here, ON DISK — nothing is compiled into
/// the binary. Resolution:
///
/// 1. `RUSHWIND_CONFIG_DIR` — **authoritative**: when set it is used
///    as-is, and a missing `data.yaml` there is a startup error (no
///    silent fallback: an operator who points at the wrong directory
///    must hear about it, never run on some other config);
/// 2. otherwise the first candidate that carries `data.yaml` wins:
///    `./configs` (deployment: run beside the configs dir),
///    `./apps/admin/configs` (repo cwd),
///    `<exe_dir>/configs` (a binary shipped with a sibling configs/),
///    `<ancestor>/apps/admin/configs` for the executable's ancestors —
///    the cargo dev layout (`target/debug/admin.exe` → `backend/apps/admin/configs`).
///
/// A miss lists every candidate it tried: the failure names itself
/// instead of guessing.
pub fn config_dir() -> Result<&'static Path, String> {
    static RESOLVED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    match RESOLVED.get_or_init(resolve_config_dir) {
        Ok(dir) => Ok(dir.as_path()),
        Err(error) => Err(error.clone()),
    }
}

fn resolve_config_dir() -> Result<PathBuf, String> {
    let override_dir = env_non_empty("RUSHWIND_CONFIG_DIR").map(PathBuf::from);
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    resolve_dir(override_dir.as_deref(), cwd.as_deref(), exe_dir.as_deref())
}

/// The pure resolution behind [`config_dir`] — `override_dir` is the
/// explicit `RUSHWIND_CONFIG_DIR` (authoritative when set), `cwd` and
/// `exe_dir` feed the deployment and cargo-dev candidates. Split out so
/// the authority rule and the candidate order are unit-testable without
/// process globals.
fn resolve_dir(
    override_dir: Option<&Path>,
    cwd: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    // The explicit override is authoritative — validated, never
    // bypassed.
    if let Some(dir) = override_dir {
        if dir.join("data.yaml").is_file() {
            return Ok(dir.to_path_buf());
        }
        return Err(format!(
            "RUSHWIND_CONFIG_DIR={} carries no data.yaml — point it at the directory \
             holding data.yaml / auth.yaml / server.yaml",
            dir.display()
        ));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(cwd) = cwd {
        candidates.push(cwd.join("configs"));
        candidates.push(cwd.join("apps/admin/configs"));
    }
    if let Some(exe_dir) = exe_dir {
        candidates.push(exe_dir.join("configs"));
        for ancestor in exe_dir.ancestors().skip(1).take(5) {
            candidates.push(ancestor.join("apps/admin/configs"));
        }
    }
    for candidate in &candidates {
        if candidate.join("data.yaml").is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "config directory not found: no candidate carries data.yaml — tried {}. \
         Set RUSHWIND_CONFIG_DIR to the directory holding data.yaml / auth.yaml / server.yaml.",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Reads a required config file from [`config_dir`]. The error carries
/// the file, the directory, and the underlying io failure.
pub fn read_config_file(name: &str) -> Result<String, String> {
    let dir = config_dir()?;
    let path = dir.join(name);
    std::fs::read_to_string(&path).map_err(|e| format!("read config file {}: {e}", path.display()))
}

/// Reads an optional config file: `None` when the file is absent, a
/// warn line (and `None`) when it is present but unreadable.
pub fn read_optional_config_file(name: &str) -> Option<String> {
    let dir = config_dir().ok()?;
    let path = dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(target: "rushwind.admin.config", %error, path = %path.display(), "optional config file unreadable");
            None
        }
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
    use super::{
        authz_kind_of, env_bool, env_int, env_non_empty, env_non_empty_or, env_string, resolve_dir,
        AuthFile, AuthzKind,
    };
    use std::env::{remove_var, set_var};
    use std::path::Path;

    /// 配置目录解析：显式 override 权威（指错目录=报错，绝不静默回落到
    /// 别的候选——运维改 A 目录却跑 B 目录是事故面）；未设时按候选链
    /// 取第一个带 data.yaml 者，含 cargo 开发布局（exe 向上找
    /// apps/admin/configs）。
    #[test]
    fn config_dir_resolution_prefers_override_and_walks_the_dev_layout() {
        let root = std::env::temp_dir().join(format!(
            "rw_cfg_probe_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        // The explicit dir: authoritative when it carries data.yaml …
        let explicit = root.join("explicit");
        std::fs::create_dir_all(&explicit).expect("mkdir explicit");
        std::fs::write(explicit.join("data.yaml"), "data: {}\n").expect("write data.yaml");
        assert_eq!(
            resolve_dir(Some(&explicit), None, None).expect("explicit dir wins"),
            explicit
        );
        // … and a hard error when it does not (no fallback to the other
        // candidates, whatever they contain).
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).expect("mkdir empty");
        let error = resolve_dir(Some(&empty), None, None).expect_err("empty override fails");
        assert!(error.contains("RUSHWIND_CONFIG_DIR"), "{error}");
        assert!(error.contains("data.yaml"), "{error}");

        // No override: the repo dev layout — an exe under
        // <root>/target/debug finds <root>/apps/admin/configs.
        let dev_configs = root.join("apps/admin/configs");
        std::fs::create_dir_all(&dev_configs).expect("mkdir dev configs");
        std::fs::write(dev_configs.join("data.yaml"), "data: {}\n").expect("write dev data.yaml");
        let exe_dir = root.join("target/debug");
        std::fs::create_dir_all(&exe_dir).expect("mkdir exe dir");
        assert_eq!(
            resolve_dir(None, Some(&root), Some(&exe_dir)).expect("dev layout found"),
            dev_configs
        );
        // A sibling configs/ next to the binary wins (deployment shape)
        // when the cwd carries nothing — the cwd candidates come first,
        // so this case pins a cwd without a configs tree.
        let bare_cwd = root.join("bare");
        std::fs::create_dir_all(&bare_cwd).expect("mkdir bare cwd");
        let sibling = exe_dir.join("configs");
        std::fs::create_dir_all(&sibling).expect("mkdir sibling configs");
        std::fs::write(sibling.join("data.yaml"), "data: {}\n").expect("write sibling data.yaml");
        assert_eq!(
            resolve_dir(None, Some(&bare_cwd), Some(&exe_dir)).expect("sibling wins"),
            sibling
        );

        // Nothing anywhere (a SECOND fresh tree, so the first root's
        // apps/admin/configs is not reachable through the ancestor
        // walk): the error lists every candidate.
        let bare_root = std::env::temp_dir().join(format!(
            "rw_cfg_bare_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let bare_exe = bare_root.join("none/debug");
        std::fs::create_dir_all(&bare_exe).expect("mkdir bare exe");
        let error = resolve_dir(None, Some(&bare_root), Some(Path::new(&bare_exe)))
            .expect_err("no candidates");
        assert!(error.contains("config directory not found"), "{error}");
        assert!(error.contains("apps/admin/configs"), "{error}");
        let _ = std::fs::remove_dir_all(&bare_root);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// auth.yaml `authz.type` → 引擎档：缺节/`zanzibar` = 阶段缺席，
    /// `""`/`noop`/未知拼写 = noop（参照 switch 的 default 臂），
    /// casbin/opa 各归其档。
    #[test]
    fn authz_section_maps_to_engine_kinds() {
        // The full auth.yaml document shape (`authn:` sibling included),
        // the section taken off the parsed file — the exact path the
        // loader walks.
        let section = |yaml: &str| -> Option<super::AuthzSection> {
            let file: AuthFile = serde_yaml::from_str(yaml).expect("parse auth.yaml");
            file.authz
        };
        assert_eq!(authz_kind_of(None), None);
        assert_eq!(
            authz_kind_of(section("authz: { type: \"noop\" }")),
            Some(AuthzKind::Noop)
        );
        assert_eq!(
            authz_kind_of(section("authz: { type: \"\" }")),
            Some(AuthzKind::Noop)
        );
        assert_eq!(
            authz_kind_of(section("authz: { type: \"casbin\" }")),
            Some(AuthzKind::Casbin)
        );
        assert_eq!(
            authz_kind_of(section("authz: { type: \"opa\" }")),
            Some(AuthzKind::Opa)
        );
        assert_eq!(
            authz_kind_of(section("authz: { type: \"zanzibar\" }")),
            None
        );
        assert_eq!(
            authz_kind_of(section("authz: { type: \"what-else\" }")),
            Some(AuthzKind::Noop)
        );
        // The vendored deployment face: the noop spelling, plus the
        // empty engine sub-sections the serde face drops.
        assert_eq!(
            authz_kind_of(section(
                "authz:\n  type: \"noop\" # casbin, opa, zanzibar, noop\n  casbin:\n  opa:\n"
            )),
            Some(AuthzKind::Noop)
        );
    }

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
