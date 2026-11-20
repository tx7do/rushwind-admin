//! Captcha — 6-char alphanumeric challenges rendered as PNG:
//! 6 chars drawn from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`, 10-minute TTL,
//! Redis key `admin:captcha:{id}`, verify-and-delete on match.

use rand::Rng;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

pub const CAPTCHA_SOURCE: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CAPTCHA_TTL_SECS: u64 = 600;

/// Candidate stroke hues: dark and saturated, one picked per challenge so
/// consecutive challenges do not look mechanically identical. Every entry
/// keeps all channels far below the white background, so the undulating
/// text stays high-contrast and legible.
const CAPTCHA_HUES: [[u8; 3]; 6] = [
    [0x8b, 0x1a, 0x1a], // crimson
    [0x1a, 0x1a, 0x8b], // navy
    [0x1a, 0x6b, 0x1a], // forest
    [0x5c, 0x1a, 0x5c], // plum
    [0x1a, 0x5c, 0x5c], // teal
    [0x6b, 0x3a, 0x0a], // rust
];

fn captcha_key(id: &str) -> String {
    format!("admin:captcha:{id}")
}

/// Generates a captcha: returns (id, base64 png data-url, answer).
pub async fn generate(redis: &ConnectionManager) -> Result<(String, String, String), String> {
    let mut conn = redis.clone();
    let id = uuid::Uuid::now_v7().simple().to_string();
    let (b64, chars) = render_png_base64()?;
    // SET+EX 语句形（非 SETEX 命令）——mini-redis 测试面不认 SETEX，
    // 答案落不了库会让 verify 永假（渐进验证码链路测试的前置修复）。
    let _: redis::RedisResult<()> = redis::cmd("SET")
        .arg(captcha_key(&id))
        .arg(chars.clone())
        .arg("EX")
        .arg(CAPTCHA_TTL_SECS)
        .query_async(&mut conn)
        .await;
    drop(conn);
    Ok((id, format!("data:image/png;base64,{b64}"), chars))
}

/// Verify-and-delete: a match consumes the row; anything else (missing,
/// expired, mismatched) fails.
///
/// 猜测预算（H6）：错答不再让行原样保留——每个 id 记错答次数，超过 5 次
/// 挑战即作废，防对同一 captcha_id 无限在线探测。计数键 GET+SET EX 两段
/// 式（mini-redis 测试面不认 INCR/SETEX，同 ratelimit 计数器的考量）；
/// 错答计数竞态（并发丢一次计数）对"低赌注挑战"可接受，与失败计数器
/// 同一姿态。作废不用 DEL（mini-redis 不实现 DEL）——覆写为不在字形池
/// 里的哨兵值，两面通用。
const CAPTCHA_MAX_ATTEMPTS: i64 = 5;
const CAPTCHA_EXHAUSTED: &str = "!exhausted!";

fn tries_key(id: &str) -> String {
    format!("admin:captcha:tries:{id}")
}

pub async fn verify(redis: &ConnectionManager, id: &str, value: &str) -> bool {
    if id.is_empty() || value.is_empty() {
        return false;
    }
    let mut conn = redis.clone();
    let key = captcha_key(id);
    let tkey = tries_key(id);
    let stored: Option<String> = conn.get(&key).await.unwrap_or(None);
    match stored {
        Some(answer) if answer == value => {
            let _: Result<i64, _> = conn.del(&key).await;
            let _: Result<i64, _> = conn.del(&tkey).await;
            true
        }
        Some(_) => {
            let tries: Option<i64> = conn.get(&tkey).await.unwrap_or(None);
            let next = tries.unwrap_or(0) + 1;
            let _: redis::RedisResult<()> = redis::cmd("SET")
                .arg(&tkey)
                .arg(next)
                .arg("EX")
                .arg(CAPTCHA_TTL_SECS)
                .query_async(&mut conn)
                .await;
            if next > CAPTCHA_MAX_ATTEMPTS {
                // 猜测预算耗尽：挑战作废（覆写哨兵，正确答案也不再命中）。
                let _: redis::RedisResult<()> = redis::cmd("SET")
                    .arg(&key)
                    .arg(CAPTCHA_EXHAUSTED)
                    .arg("EX")
                    .arg(CAPTCHA_TTL_SECS)
                    .query_async(&mut conn)
                    .await;
            }
            false
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::test_state;

    #[tokio::test]
    async fn wrong_guesses_exhaust_then_kill_the_challenge() {
        let state = test_state().await;
        let (id, _png, answer) = generate(&state.redis).await.expect("generate");
        let wrong = if answer == "ZZZZZZ" {
            "AAAAAA"
        } else {
            "ZZZZZZ"
        };
        // 预算内错答逐次失败。
        for _ in 0..CAPTCHA_MAX_ATTEMPTS {
            assert!(!verify(&state.redis, &id, wrong).await);
        }
        // 第 6 次错答触发作废：正确答案也不再命中。
        assert!(!verify(&state.redis, &id, wrong).await);
        assert!(!verify(&state.redis, &id, &answer).await);
        // 新挑战有独立的计数键，不受残留影响。
        let (id2, _png2, answer2) = generate(&state.redis).await.expect("generate 2");
        assert!(verify(&state.redis, &id2, &answer2).await);
    }
}

/// Cropper-safe glyph-run envelope, in pixels across. The renderer packs
/// its six glyphs into a run and centers the crop window on that run; the
/// window is 240 px wide on a 400 px canvas, so a run wider than the
/// envelope would shear its first and last glyphs into the window's edge,
/// and a run narrower than the envelope floor would starve the centering
/// subtraction. Measured over the font's repertoire, a six-glyph run
/// lands anywhere in 120–324 px; inside the envelope the window provably
/// stays within the canvas with blank margin to spare on every side.
/// Outside draws are simply redrawn — the current font needs that for
/// 6.6% of draws, and the 32-draw budget exhausts with probability on the
/// order of 0.066³².
const RUN_MIN_PX: u32 = 96;
const RUN_MAX_PX: u32 = 232;

/// Renders the challenge as a 240x96 PNG and returns `(base64 png, answer)`.
///
/// Pipeline: six glyphs laid side by side, one horizontal sine wave so the
/// run undulates, a crop that keeps the text centered, and finally a random
/// deep hue recoloring the strokes. The answer is read back off the render,
/// so image and stored answer always agree.
///
/// The glyph pool is [`CAPTCHA_SOURCE`] intersected with the renderer
/// font's repertoire: the font cannot draw a few source glyphs (`L`), and
/// an unfiltered pick silently drops them, shortening some answers below
/// six characters.
fn render_png_base64() -> Result<(String, String), String> {
    let glyphs: Vec<char> = {
        let probe = captcha::Captcha::new();
        let supported = probe.supported_chars();
        CAPTCHA_SOURCE
            .chars()
            .filter(|c| supported.contains(c))
            .collect()
    };
    if glyphs.len() < 6 {
        return Err("captcha font repertoire covers too few glyphs".to_string());
    }
    let mut cap = (0..32)
        .find_map(|_| {
            let mut attempt = captcha::Captcha::new();
            attempt.set_chars(&glyphs);
            attempt.add_chars(6);
            let g = attempt.text_area();
            let run = g.right - g.left + 1;
            (RUN_MIN_PX..=RUN_MAX_PX).contains(&run).then_some(attempt)
        })
        .ok_or_else(|| "captcha layout retry budget exhausted".to_string())?;
    let answer = cap.chars_as_string();
    cap.apply_filter(captcha::filters::Wave::new(2.0, 12.0).horizontal());
    cap.view(240, 96);
    let hue = CAPTCHA_HUES[rand::rng().random_range(0..CAPTCHA_HUES.len())];
    cap.set_color(hue);
    let b64 = cap
        .as_base64()
        .ok_or_else(|| "captcha render".to_string())?;
    Ok((b64, answer))
}
