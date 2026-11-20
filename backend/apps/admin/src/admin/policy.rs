//! Pure policy matchers — login-policy
//! helpers and the password-complexity classifier.

/// `matchIPValue`: exact IP or CIDR range (v4).
pub fn ip_matches(client_ip: &str, value: &str) -> bool {
    if client_ip.is_empty() {
        return false;
    }
    let (addr, mask) = match value.split_once('/') {
        Some((addr, mask)) => (addr, mask.parse::<u32>().unwrap_or(32)),
        None => return client_ip == value,
    };
    let (Some(client), Some(base)) = (ipv4_to_u32(client_ip), ipv4_to_u32(addr)) else {
        return false;
    };
    if mask == 0 {
        return true;
    }
    if mask > 32 {
        return false;
    }
    let prefix = u32::MAX << (32 - mask);
    (client & prefix) == (base & prefix)
}

fn ipv4_to_u32(text: &str) -> Option<u32> {
    // 逐段全解析（非 filter_map 静默丢弃）：原实现 "1.2.3.4.5" 会把
    // 末段丢掉后按 1.2.3.4 误匹配，畸形策略值退化成意外命中。
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let octets = parts
        .iter()
        .map(|p| p.parse::<u8>().ok())
        .collect::<Option<Vec<u8>>>()?;
    Some(
        ((octets[0] as u32) << 24)
            | ((octets[1] as u32) << 16)
            | ((octets[2] as u32) << 8)
            | octets[3] as u32,
    )
}

/// `matchTimeWindow`: `HH:MM-HH:MM` UTC window (end inclusive) — window
/// specs are authored against the system-wide timezone convention (UTC),
/// not the host clock.
/// 跨午夜窗口（start > end，如 22:00-06:00）语义为"越过午夜仍连续"——
/// 原实现恒 false，用作白名单等于全天拒绝、用作黑名单夜间封禁静默失效。
pub fn time_window_matches(value: &str) -> bool {
    let utc = chrono::Utc::now();
    time_window_matches_at(value, utc.hour(), utc.minute())
}

/// The pure matcher behind [`time_window_matches`], taking the clock
/// so the window logic (inclusive ends, midnight wrap) is testable.
pub fn time_window_matches_at(value: &str, hour: u32, minute: u32) -> bool {
    let Some((start, end)) = value.split_once('-') else {
        return false;
    };
    let minutes = |text: &str| -> Option<u32> {
        let (h, m) = text.trim().split_once(':')?;
        Some(h.trim().parse::<u32>().ok()? * 60 + m.trim().parse::<u32>().ok()?)
    };
    let (Some(start), Some(end)) = (minutes(start), minutes(end)) else {
        return false;
    };
    let now = hour * 60 + minute;
    if start <= end {
        now >= start && now <= end
    } else {
        now >= start || now <= end
    }
}

use chrono::Timelike;

/// The password char-class count: lower/upper/digit/special — the
/// ≥3-classes complexity rule.
pub fn char_classes(password: &str) -> u32 {
    let mut classes = 0u32;
    let (mut lower, mut upper, mut digit, mut special) = (false, false, false, false);
    for ch in password.chars() {
        match ch {
            'a'..='z' => lower = true,
            'A'..='Z' => upper = true,
            '0'..='9' => digit = true,
            _ => special = true,
        }
    }
    for hit in [lower, upper, digit, special] {
        if hit {
            classes += 1;
        }
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_exact_match_and_mismatch() {
        assert!(ip_matches("10.0.0.1", "10.0.0.1"));
        assert!(!ip_matches("10.0.0.1", "10.0.0.2"));
        assert!(!ip_matches("", "10.0.0.1"));
        assert!(!ip_matches("not-an-ip", "10.0.0.1"));
    }

    #[test]
    fn ip_cidr_membership() {
        assert!(ip_matches("192.168.4.5", "192.168.4.0/24"));
        assert!(!ip_matches("192.168.5.5", "192.168.4.0/24"));
        assert!(!ip_matches("192.168.4.5", "192.168.4.0/33"));
        assert!(!ip_matches("192.168.4.5", "not-an-ip/24"));
    }

    #[test]
    fn ip_zero_mask_is_wildcard() {
        assert!(ip_matches("1.2.3.4", "0.0.0.0/0"));
    }

    #[test]
    fn ip_rejects_malformed_values() {
        // 逐段全解析回归：多段/缺段/空段不得静默降级成误匹配。
        assert!(!ip_matches("1.2.3.4", "1.2.3.4.5"));
        assert!(!ip_matches("1.2.3.4", "1..2.3"));
        assert!(!ip_matches("1.2.3.4", "1.2.3"));
        assert!(!ip_matches("1.2.3.4", "300.1.1.1"));
    }

    #[test]
    fn time_window_rejects_malformed() {
        assert!(!time_window_matches("nodash"));
        assert!(!time_window_matches("9:00-not-a-time"));
        assert!(!time_window_matches("bad:00-10:00"));
    }

    #[test]
    fn time_window_same_day_bounds() {
        // 端点均含：22:00 前/后不入窗，22:00 与 23:59 入窗。
        assert!(!time_window_matches_at("22:00-23:59", 21, 59));
        assert!(time_window_matches_at("22:00-23:59", 22, 0));
        assert!(time_window_matches_at("22:00-23:59", 23, 59));
    }

    #[test]
    fn time_window_wraps_midnight() {
        // 跨午夜窗口（22:00-06:00）：两侧深夜入窗、正午不入窗、午夜整点入窗。
        assert!(time_window_matches_at("22:00-06:00", 23, 30));
        assert!(time_window_matches_at("22:00-06:00", 5, 59));
        assert!(time_window_matches_at("22:00-06:00", 6, 0));
        assert!(!time_window_matches_at("22:00-06:00", 12, 0));
        assert!(time_window_matches_at("22:00-06:00", 0, 0));
    }

    #[test]
    fn char_class_counting() {
        assert_eq!(char_classes("abcdef"), 1);
        assert_eq!(char_classes("aBc"), 2);
        assert_eq!(char_classes("aB1"), 3);
        assert_eq!(char_classes("aB1!"), 4);
        assert_eq!(char_classes(""), 0);
    }
}
