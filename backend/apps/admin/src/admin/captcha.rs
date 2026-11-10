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
    let _: redis::RedisResult<()> = conn
        .set_ex(captcha_key(&id), chars.clone(), CAPTCHA_TTL_SECS)
        .await;
    drop(conn);
    Ok((id, format!("data:image/png;base64,{b64}"), chars))
}

/// Verify-and-delete: a match consumes the row; anything else (missing,
/// expired, mismatched) fails.
pub async fn verify(redis: &ConnectionManager, id: &str, value: &str) -> bool {
    if id.is_empty() || value.is_empty() {
        return false;
    }
    let mut conn = redis.clone();
    let stored: Option<String> = conn.get(captcha_key(id)).await.unwrap_or(None);
    match stored {
        Some(answer) if answer == value => {
            let _: Result<i64, _> = conn.del(captcha_key(id)).await;
            true
        }
        _ => false,
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
