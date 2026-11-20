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

/// The progressive-captcha counter (issue #1): after `threshold` failures
/// for the same (client_ip, username), logins must present a captcha.
/// Deliberately GET+SET EX (no EVAL/INCR/DEL/EXPIRE): the test face runs
/// mini-redis, which implements only get/publish/set/subscribe — the Lua
/// path above is untestable there, this one is. The race window (two
/// concurrent failures losing one count) is acceptable for a low-stakes
/// upgrade like "show the captcha box".
/// Captcha-failure keys, same two dimensions as the lockout keys (empty
/// dimension contributes no key — no match-all).
fn captcha_fail_keys(ip: &str, username: &str) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    if !ip.is_empty() {
        keys.push(format!("admin:login:captcha:ip:{ip}"));
    }
    if !username.is_empty() {
        keys.push(format!("admin:login:captcha:user:{username}"));
    }
    keys
}

/// Should this login attempt present a captcha? Pure read, fail-open
/// (Redis trouble never locks a legitimate user out of the no-captcha
/// fast path; the lockout gate above still protects the account).
pub async fn captcha_required(
    redis: &ConnectionManager,
    ip: &str,
    username: &str,
    threshold: i64,
) -> bool {
    let mut conn = redis.clone();
    for key in captcha_fail_keys(ip, username) {
        let count: Option<i64> = match conn.get(&key).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if count.unwrap_or(0) >= threshold {
            return true;
        }
    }
    false
}

/// Record one credential failure for the captcha counter. Sliding window:
/// every failure re-arms the TTL.
pub async fn incr_captcha_fail(redis: &ConnectionManager, ip: &str, username: &str, ttl_secs: i64) {
    let mut conn = redis.clone();
    for key in captcha_fail_keys(ip, username) {
        let current: Option<i64> = conn.get(&key).await.ok().flatten();
        let next = current.unwrap_or(0) + 1;
        // SET+EX 语句形（非 SETEX 命令）——与 token store 同一 mini-redis 兼容考量。
        redis::cmd("SET")
            .arg(&key)
            .arg(next)
            .arg("EX")
            .arg(ttl_secs)
            .query_async::<()>(&mut conn)
            .await
            .ok();
    }
}

/// Credential success clears the captcha counter (both dimensions).
/// SET+EX 语句形（写 0 值）——SETEX 在 mini-redis 测试面是 no-op，清零
/// 会失效残留旧计数。
pub async fn reset_captcha_fail(redis: &ConnectionManager, ip: &str, username: &str) {
    let mut conn = redis.clone();
    for key in captcha_fail_keys(ip, username) {
        redis::cmd("SET")
            .arg(&key)
            .arg(0i64)
            .arg("EX")
            .arg(15 * 60)
            .query_async::<()>(&mut conn)
            .await
            .ok();
    }
}

/// The reset-code verify limiter (忘记密码链路): every verification
/// attempt counts per identifier; past the cap the endpoint keeps
/// answering "invalid verification code" until the window expires and a
/// fresh ForgotPassword code is requested. GET+SET EX two-form — the
/// mini-redis test face implements neither INCR nor SETEX (same
/// constraint as the captcha counter above); fail-open on Redis errors
/// (next resets to 1).
const RESET_CODE_MAX_ATTEMPTS: i64 = 5;
const RESET_CODE_WINDOW_SECS: i64 = 600;

pub async fn reset_code_attempt(redis: &ConnectionManager, identifier: &str) -> bool {
    let mut conn = redis.clone();
    let key = format!("admin:reset:attempts:{identifier}");
    let current: Option<i64> = conn.get(&key).await.ok().flatten();
    let next = current.unwrap_or(0) + 1;
    redis::cmd("SET")
        .arg(&key)
        .arg(next)
        .arg("EX")
        .arg(RESET_CODE_WINDOW_SECS)
        .query_async::<()>(&mut conn)
        .await
        .ok();
    next > RESET_CODE_MAX_ATTEMPTS
}
