//! The Redis login rate limiter: 5 failures / 15 min window,
//! per-IP and per-username keys, Lua atomic incr-if-not-locked,
//! fail-open on Redis errors.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

const LOGIN_FAIL_THRESHOLD: i64 = 5;
const LOGIN_LOCKOUT_SECS: i64 = 15 * 60;

fn fail_keys(ip: &str, username: &str) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    if !ip.is_empty() {
        keys.push(format!("admin:login:fail:ip:{ip}"));
    }
    if !username.is_empty() {
        keys.push(format!("admin:login:fail:user:{username}"));
    }
    keys
}

/// IsLocked — the pre-login gate (no counter increment).
pub async fn is_locked(redis: &ConnectionManager, ip: &str, username: &str) -> bool {
    let mut conn = redis.clone();
    for key in fail_keys(ip, username) {
        let count: Option<i64> = match conn.get(&key).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if count.unwrap_or(0) >= LOGIN_FAIL_THRESHOLD {
            return true;
        }
    }
    false
}

/// CheckAndIncr — called on login failure; returns whether either
/// dimension has hit the lockout threshold.
pub async fn check_and_incr(redis: &ConnectionManager, ip: &str, username: &str) -> bool {
    let mut conn = redis.clone();
    let script = redis::Script::new(
        r#"
        local key = KEYS[1]
        local threshold = tonumber(ARGV[1])
        local ttl = tonumber(ARGV[2])
        local current = tonumber(redis.call('GET', key) or "0")
        if current >= threshold then
            return {1, current}
        end
        current = redis.call('INCR', key)
        if current == 1 then
            redis.call('EXPIRE', key, ttl)
        end
        return {0, current}
    "#,
    );
    for key in fail_keys(ip, username) {
        let result: Option<(i32, i32)> = script
            .key(&key)
            .arg(LOGIN_FAIL_THRESHOLD)
            .arg(LOGIN_LOCKOUT_SECS)
            .invoke_async(&mut conn)
            .await
            .ok();
        if let Some((locked, _)) = result {
            if locked == 1 {
                return true;
            }
        }
    }
    false
}

/// Reset — cleared on login success.
pub async fn reset(redis: &ConnectionManager, ip: &str, username: &str) {
    let mut conn = redis.clone();
    let keys = fail_keys(ip, username);
    if !keys.is_empty() {
        let _: Result<i64, _> = conn.del(&keys).await;
    }
}
