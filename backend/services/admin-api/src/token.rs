//! Token issuance + the Redis session/token cache.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde_json::{json, Map, Value};

/// `ClientType_admin` — the REST surface's client type enum value.
pub const CLIENT_TYPE_ADMIN: u32 = 0;

/// UserTokenPayload — the typed view over the claims bag

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UserTokenPayload {
    pub username: String,
    pub user_id: u32,
    pub tenant_id: u32,
    pub org_unit_id: Option<u32>,
    pub roles: Vec<String>,
    pub client_id: Option<String>,
    pub device_id: Option<String>,
    pub data_scope: Option<String>,
    pub data_scopes: Vec<String>,
    /// Comma-joined decimal uint64 units (the `dsu` wire form).
    pub data_scope_unit_ids: Option<String>,
    pub hidden_fields: Vec<String>,
    pub is_platform_admin: Option<bool>,
    pub is_tenant_admin: Option<bool>,
    pub jti: String,
}

impl UserTokenPayload {
    pub fn from_claims(claims: &Map<String, Value>) -> Option<Self> {
        let get_u32 = |key: &str| -> Option<u32> {
            match claims.get(key)? {
                Value::Number(n) => n.as_u64().map(|v| v as u32),
                Value::String(s) => s.parse().ok(),
                _ => None,
            }
        };
        let get_str = |key: &str| -> Option<String> {
            claims.get(key).and_then(|v| v.as_str()).map(String::from)
        };
        let get_strs = |key: &str| -> Vec<String> {
            match claims.get(key) {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let get_bool = |key: &str| -> Option<bool> {
            match claims.get(key)? {
                Value::Bool(b) => Some(*b),
                Value::String(s) => Some(s == "true"),
                _ => None,
            }
        };
        Some(UserTokenPayload {
            username: get_str("sub")?,
            user_id: get_u32("uid")?,
            tenant_id: get_u32("tid").unwrap_or(0),
            org_unit_id: get_u32("ouid"),
            roles: get_strs("roc"),
            client_id: get_str("cid"),
            device_id: get_str("did"),
            data_scope: get_str("ds"),
            data_scopes: get_strs("dss"),
            data_scope_unit_ids: get_str("dsu"),
            hidden_fields: get_strs("hfs"),
            is_platform_admin: get_bool("ipa"),
            is_tenant_admin: get_bool("ita"),
            jti: get_str("jti").unwrap_or_default(),
        })
    }

    /// The access-claim bag — `NewUserTokenAuthClaims` (claim names
    /// verbatim; `tid` always present, 0 = platform).
    pub fn to_access_claims(&self, exp_unix: i64) -> Map<String, Value> {
        let mut claims = Map::new();
        claims.insert("sub".into(), json!(self.username));
        claims.insert("uid".into(), json!(self.user_id));
        claims.insert("tid".into(), json!(self.tenant_id));
        claims.insert("iat".into(), json!(chrono::Utc::now().timestamp()));
        claims.insert("exp".into(), json!(exp_unix));
        claims.insert("jti".into(), json!(self.jti));
        if !self.roles.is_empty() {
            claims.insert("roc".into(), json!(self.roles));
        }
        if let Some(device) = &self.device_id {
            claims.insert("did".into(), json!(device));
        }
        if let Some(client) = &self.client_id {
            claims.insert("cid".into(), json!(client));
        }
        if let Some(scope) = &self.data_scope {
            claims.insert("ds".into(), json!(scope));
        }
        if !self.data_scopes.is_empty() {
            claims.insert("dss".into(), json!(self.data_scopes));
        }
        if let Some(units) = &self.data_scope_unit_ids {
            claims.insert("dsu".into(), json!(units));
        }
        if !self.hidden_fields.is_empty() {
            claims.insert("hfs".into(), json!(self.hidden_fields));
        }
        if let Some(ouid) = self.org_unit_id {
            claims.insert("ouid".into(), json!(ouid));
        }
        if let Some(ipa) = self.is_platform_admin {
            claims.insert("ipa".into(), json!(ipa));
        }
        if let Some(ita) = self.is_tenant_admin {
            claims.insert("ita".into(), json!(ita));
        }
        claims
    }

    /// The refresh-claim bag: uid + jti + iat + exp only
    pub fn to_refresh_claims(&self, exp_unix: i64) -> Map<String, Value> {
        Map::from_iter([
            ("uid".to_string(), json!(self.user_id)),
            ("jti".to_string(), json!(self.jti)),
            ("iat".to_string(), json!(chrono::Utc::now().timestamp())),
            ("exp".to_string(), json!(exp_unix)),
        ])
    }
}

/// 32-char hex of a UUIDv7 — `jwtutil.NewJWTId`.
pub fn new_jwt_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

/// The Redis session/token cache. Key shapes verbatim from

#[derive(Clone)]
pub struct TokenStore {
    redis: ConnectionManager,
    pub access_expires_secs: i64,
    pub refresh_expires_secs: i64,
}

fn at_key(ct: u32, uid: u32, jti: &str) -> String {
    format!("at:{ct}:{uid}:{jti}")
}

fn rt_key(ct: u32, uid: u32, jti: &str) -> String {
    format!("rt:{ct}:{uid}:{jti}")
}

fn us_key(ct: u32, uid: u32, jti: &str) -> String {
    format!("us:{ct}:{uid}:{jti}")
}

fn bl_key(jti: &str) -> String {
    format!("bl:{jti}")
}

/// SessionMeta — the `us:` row content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMeta {
    #[serde(rename = "username")]
    pub username: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: u32,
    #[serde(rename = "ip")]
    pub ip: String,
    #[serde(rename = "ua")]
    pub user_agent: String,
    #[serde(rename = "dev", skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(rename = "loginAt")]
    pub login_at: String,
}

/// Collects one SCAN pattern's keys; `None` when the scan itself fails.
/// The cursor drops before the caller reuses the connection.
async fn scan_keys(conn: &mut ConnectionManager, pattern: String) -> Option<Vec<String>> {
    let mut iter = conn.scan_match::<_, String>(pattern).await.ok()?;
    let mut keys = Vec::new();
    while let Some(Ok(key)) = iter.next_item().await {
        keys.push(key);
    }
    drop(iter);
    Some(keys)
}

impl TokenStore {
    pub fn new(
        redis: ConnectionManager,
        access_expires_secs: i64,
        refresh_expires_secs: i64,
    ) -> Self {
        Self {
            redis,
            access_expires_secs,
            refresh_expires_secs,
        }
    }

    /// AddTokenPair — TxPipeline SET at:/rt: with their TTLs.
    pub async fn add_token_pair(
        &self,
        uid: u32,
        jti: &str,
        access_token: &str,
        refresh_token: &str,
    ) -> Result<(), String> {
        let mut conn = self.redis.clone();
        redis::pipe()
            .set_ex(
                at_key(CLIENT_TYPE_ADMIN, uid, jti),
                access_token,
                self.access_expires_secs as u64,
            )
            .set_ex(
                rt_key(CLIENT_TYPE_ADMIN, uid, jti),
                refresh_token,
                self.refresh_expires_secs as u64,
            )
            .query_async::<()>(&mut conn)
            .await
            .map_err(|e| format!("redis add token pair: {e}"))?;
        Ok(())
    }

    /// AddAccessToken — machine tokens (uid 0) ride the same whitelist.
    pub async fn add_access_token(
        &self,
        uid: u32,
        jti: &str,
        access_token: &str,
    ) -> Result<(), String> {
        let mut conn = self.redis.clone();
        conn.set_ex(
            at_key(CLIENT_TYPE_ADMIN, uid, jti),
            access_token,
            self.access_expires_secs as u64,
        )
        .await
        .map_err(|e| format!("redis add access token: {e}"))
    }

    /// IsValidAccessToken — exact stored-token compare.
    pub async fn is_valid_access_token(&self, uid: u32, jti: &str, token: &str) -> bool {
        let mut conn = self.redis.clone();
        let stored: Option<String> = conn.get(at_key(CLIENT_TYPE_ADMIN, uid, jti)).await.ok();
        stored.as_deref() == Some(token)
    }

    /// IsBlockedAccessToken — EXISTS on bl:{jti}.
    pub async fn is_blocked_access_token(&self, jti: &str) -> bool {
        let mut conn = self.redis.clone();
        conn.exists::<_, bool>(bl_key(jti)).await.unwrap_or(false)
    }

    /// IsExistRefreshToken — the refresh whitelist row for (uid, token).
    /// Wired with the online-session service (storage phase).
    #[allow(dead_code)]
    pub async fn refresh_token_matches(&self, uid: u32, token: &str) -> Option<String> {
        let mut conn = self.redis.clone();
        let _jti_out = String::new();
        // The jti is inside the token itself; scan this user's rt: rows.
        let pattern = format!("rt:{CLIENT_TYPE_ADMIN}:{uid}:*");
        let keys = scan_keys(&mut conn, pattern).await?;
        for key in keys {
            let stored: Option<String> = conn.get(&key).await.ok();
            if stored.as_deref() == Some(token) {
                return key.rsplit(':').next().map(String::from);
            }
        }
        None
    }

    /// The Lua verify-and-revoke: GET rt == token → DEL rt/at/us.
    pub async fn verify_and_revoke_refresh_token(&self, uid: u32, jti: &str, token: &str) -> bool {
        let mut conn = self.redis.clone();
        let script = redis::Script::new(
            r#"
            local rtKey = KEYS[1]
            local atKey = KEYS[2]
            local usKey = KEYS[3]
            local refreshToken = ARGV[1]
            local stored = redis.call('GET', rtKey)
            if not stored or stored ~= refreshToken then
                return 0
            end
            redis.call('DEL', rtKey)
            redis.call('DEL', atKey)
            if usKey and usKey ~= '' then
                redis.call('DEL', usKey)
            end
            return 1
        "#,
        );
        let result: i32 = script
            .key(rt_key(CLIENT_TYPE_ADMIN, uid, jti))
            .key(at_key(CLIENT_TYPE_ADMIN, uid, jti))
            .key(us_key(CLIENT_TYPE_ADMIN, uid, jti))
            .arg(token)
            .invoke_async(&mut conn)
            .await
            .unwrap_or(0);
        result == 1
    }

    /// RecordSessionMeta — us:{ct}:{uid}:{jti} JSON, refresh TTL.
    pub async fn set_session_meta(
        &self,
        uid: u32,
        jti: &str,
        meta: &SessionMeta,
    ) -> Result<(), String> {
        let mut conn = self.redis.clone();
        let body = serde_json::to_string(meta).map_err(|e| e.to_string())?;
        conn.set_ex(
            us_key(CLIENT_TYPE_ADMIN, uid, jti),
            body,
            self.refresh_expires_secs as u64,
        )
        .await
        .map_err(|e| format!("redis set session meta: {e}"))
    }

    /// Wired with the online-session service (storage phase).
    #[allow(dead_code)]
    pub async fn get_session_meta(&self, uid: u32, jti: &str) -> Option<SessionMeta> {
        let mut conn = self.redis.clone();
        let raw: Option<String> = conn.get(us_key(CLIENT_TYPE_ADMIN, uid, jti)).await.ok()?;
        serde_json::from_str(raw.as_deref()?).ok()
    }

    /// All online sessions for a user (us:0:{uid}:* → (jti, meta)).
    pub async fn list_user_sessions(&self, uid: u32) -> Vec<(String, SessionMeta)> {
        let mut conn = self.redis.clone();
        let pattern = format!("us:{CLIENT_TYPE_ADMIN}:{uid}:*");
        let keys = scan_keys(&mut conn, pattern).await.unwrap_or_default();
        let mut out = Vec::new();
        for key in keys {
            let raw: Option<String> = conn.get(&key).await.ok();
            if let Some(meta) = raw.as_deref().and_then(|r| serde_json::from_str(r).ok()) {
                let jti = key.rsplit(':').next().unwrap_or_default().to_string();
                out.push((jti, meta));
            }
        }
        out
    }

    /// ALL online sessions (us:* → (ct, uid, jti, meta)) — admin listing.
    pub async fn list_all_sessions(&self) -> Vec<(u32, u32, String, SessionMeta)> {
        let mut conn = self.redis.clone();
        let all_keys = scan_keys(&mut conn, "us:*".to_string())
            .await
            .unwrap_or_default();
        let mut out = Vec::new();
        for key in all_keys {
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() != 4 {
                continue;
            }
            let (Ok(ct), Ok(uid)) = (parts[1].parse::<u32>(), parts[2].parse::<u32>()) else {
                continue;
            };
            let raw: Option<String> = conn.get(&key).await.ok();
            if let Some(meta) = raw.as_deref().and_then(|r| serde_json::from_str(r).ok()) {
                out.push((ct, uid, parts[3].to_string(), meta));
            }
        }
        out
    }

    /// RevokeToken — SCAN-delete at/rt/us rows of (ct, uid).
    pub async fn revoke_user_token(&self, uid: u32) {
        let mut conn = self.redis.clone();
        for prefix in ["at", "rt", "us"] {
            let pattern = format!("{prefix}:{CLIENT_TYPE_ADMIN}:{uid}:*");
            let Some(keys) = scan_keys(&mut conn, pattern).await else {
                continue;
            };
            if !keys.is_empty() {
                let _: Result<i64, _> = conn.del(&keys).await;
            }
        }
    }

    /// RevokeTokenByJti across both client types.
    pub async fn revoke_token_by_jti(&self, uid: u32, jti: &str) {
        let mut conn = self.redis.clone();
        let _: Result<i64, _> = conn
            .del(&[
                at_key(CLIENT_TYPE_ADMIN, uid, jti),
                rt_key(CLIENT_TYPE_ADMIN, uid, jti),
                us_key(CLIENT_TYPE_ADMIN, uid, jti),
            ])
            .await;
    }

    /// RevokeUserTokenAllClientTypes — the REST surface has admin only,
    /// Client-type sweeping mirrors the all-client-types revoke.
    /// Wired with the online-session service (storage phase).
    #[allow(dead_code)]
    pub async fn revoke_user_token_all_client_types(&self, uid: u32) {
        self.revoke_user_token(uid).await;
    }

    /// Set-Cookie pair — `setRefreshCookies`. Returns (refresh_token
    /// cookie, refresh_exp cookie) header values.
    pub fn refresh_cookie_values(&self, refresh_token: &str, secure: bool) -> (String, String) {
        let max_age = self.refresh_expires_secs;
        let secure_flag = if secure { "; Secure" } else { "" };
        let refresh_cookie = format!(
            "refresh_token={refresh_token}; Path=/admin/v1/refresh-token; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure_flag}"
        );
        let exp = chrono::Utc::now().timestamp() + max_age;
        let exp_cookie =
            format!("refresh_exp={exp}; Path=/; Max-Age={max_age}; SameSite=Lax{secure_flag}");
        (refresh_cookie, exp_cookie)
    }

    /// The clear pair — Max-Age=-1 (clearRefreshCookies).
    pub fn clear_cookie_values(secure: bool) -> (String, String) {
        let secure_flag = if secure { "; Secure" } else { "" };
        (
            format!("refresh_token=; Path=/admin/v1/refresh-token; Max-Age=-1; HttpOnly; SameSite=Lax{secure_flag}"),
            format!("refresh_exp=; Path=/; Max-Age=-1; SameSite=Lax{secure_flag}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::UserTokenPayload;
    use serde_json::{json, Map, Value};

    fn base_claims() -> Map<String, Value> {
        json!({"sub": "u", "uid": "7", "tid": "3", "jti": "j1"})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn parses_required_claims() {
        let p = UserTokenPayload::from_claims(&base_claims()).unwrap();
        assert_eq!(p.username, "u");
        assert_eq!(p.user_id, 7);
        assert_eq!(p.tenant_id, 3);
    }

    #[test]
    fn numeric_uid_string_uid_both_parse() {
        let mut c = base_claims();
        c.insert("uid".into(), json!(9));
        assert_eq!(UserTokenPayload::from_claims(&c).unwrap().user_id, 9);
        c.insert("uid".into(), json!("11"));
        assert_eq!(UserTokenPayload::from_claims(&c).unwrap().user_id, 11);
    }

    #[test]
    fn missing_sub_or_uid_rejects() {
        let mut c = base_claims();
        c.remove("sub");
        assert!(UserTokenPayload::from_claims(&c).is_none());
        let mut c = base_claims();
        c.remove("uid");
        assert!(UserTokenPayload::from_claims(&c).is_none());
    }

    #[test]
    fn missing_tid_defaults_to_platform() {
        let mut c = base_claims();
        c.remove("tid");
        assert_eq!(UserTokenPayload::from_claims(&c).unwrap().tenant_id, 0);
    }
}

#[cfg(test)]
mod claims_roundtrip {
    use super::*;

    fn sample() -> UserTokenPayload {
        UserTokenPayload {
            username: "admin".into(),
            user_id: 1,
            tenant_id: 0,
            org_unit_id: Some(9),
            roles: vec!["platform:admin".into()],
            client_id: Some("web".into()),
            device_id: Some("pc-1".into()),
            data_scope: Some("ALL".into()),
            data_scopes: vec!["ALL".into()],
            data_scope_unit_ids: Some("1,2,3".into()),
            hidden_fields: vec!["salary".into()],
            is_platform_admin: Some(true),
            is_tenant_admin: Some(false),
            jti: "jti-1".into(),
        }
    }

    #[test]
    fn access_and_refresh_claims_round_trip() {
        let payload = sample();
        let access = payload.to_access_claims(1_800_000_000);
        let back = UserTokenPayload::from_claims(&access).expect("access claims reparse");
        assert_eq!(back.username, "admin");
        assert_eq!(back.user_id, 1);
        assert_eq!(back.tenant_id, 0);
        assert_eq!(back.roles, vec!["platform:admin".to_string()]);
        assert_eq!(back.data_scope.as_deref(), Some("ALL"));
        assert_eq!(back.hidden_fields, vec!["salary".to_string()]);
        assert_eq!(back.is_platform_admin, Some(true));
        assert_eq!(back.jti, "jti-1");

        // 刷新载荷只带 uid/jti/iat/exp（精简形态，from_claims 需 sub 不适用）
        let refresh = payload.to_refresh_claims(1_800_086_400);
        assert_eq!(refresh["uid"], 1);
        assert_eq!(refresh["jti"], "jti-1");
        assert_eq!(refresh["exp"], 1_800_086_400);
        assert!(refresh.get("iat").is_some());
        assert!(refresh.get("sub").is_none());
    }
}
