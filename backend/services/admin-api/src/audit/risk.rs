//! The login-risk scoring heuristics — pure functions over the request
//! facts the audit trail records (the reference risk-engine port:
//! failure weighting, unknown-user and missing-device penalties, and
//! the network-origin factor).

use std::collections::BTreeSet;

pub(super) fn is_private_ip(ip: &str) -> bool {
    let octets: Vec<u8> = ip
        .trim()
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    if octets.len() == 4 {
        let (a, b) = (octets[0], octets[1]);
        return a == 10
            || a == 127
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168)
            || (a == 169 && b == 254);
    }
    ip.starts_with("[::1]") || ip == "::1" || ip.starts_with("fc") || ip.starts_with("fd")
}

/// Risk score computation (0-100).
pub(super) fn risk_score(
    failed: bool,
    user_id: i64,
    username: &str,
    ip: &str,
    has_device: bool,
) -> i32 {
    let mut score = 0;
    if failed {
        score += 50;
    }
    if user_id == 0 {
        score += if username.is_empty() { 20 } else { 10 };
    }
    if !has_device {
        score += 10;
    }
    if ip.is_empty() {
        score += 5;
    } else if is_private_ip(ip) {
        score -= 10;
    }
    score.clamp(0, 100)
}

pub(super) fn risk_level(score: u32) -> &'static str {
    match score {
        0..=30 => "LOW",
        31..=70 => "MEDIUM",
        _ => "HIGH",
    }
}

/// Risk factors — deduped and sorted.
#[allow(clippy::too_many_arguments)]
pub(super) fn risk_factors(
    failed: bool,
    user_id: i64,
    username: &str,
    ip: &str,
    has_device: bool,
    mfa_status: &str,
    failure_reason: &str,
    request_id: &str,
    score: u32,
) -> Vec<String> {
    let mut set = BTreeSet::new();
    if failed {
        set.insert("FAILED_LOGIN");
    }
    if user_id == 0 {
        if username.is_empty() {
            set.insert("ANONYMOUS_LOGIN");
        } else {
            set.insert("UNKNOWN_USER");
        }
    }
    if !has_device {
        set.insert("UNKNOWN_DEVICE");
    }
    let mfa = mfa_status.to_uppercase();
    if mfa.contains("FAILED") {
        set.insert("MFA_FAILED");
    }
    if mfa.contains("UNVERIFY") {
        set.insert("MFA_UNVERIFIED");
    }
    if ip.is_empty() {
        set.insert("IP_MISSING");
    } else if is_private_ip(ip) {
        set.insert("INTERNAL_IP");
    } else {
        set.insert("EXTERNAL_IP");
    }
    let fr = failure_reason.to_lowercase();
    if !fr.is_empty() {
        if fr.contains("password") || fr.contains("pwd") || fr.contains("incorrect") {
            set.insert("PASSWORD_FAILURE");
        }
        if fr.contains("mfa") {
            set.insert("MFA_FAILURE_REASON");
        }
    }
    set.insert("NO_SESSION");
    if request_id.is_empty() {
        set.insert("NO_REQUEST_ID");
    }
    match score {
        71..=100 => {
            set.insert("HIGH_RISK_SCORE");
        }
        31..=70 => {
            set.insert("MEDIUM_RISK_SCORE");
        }
        1..=30 => {
            set.insert("LOW_RISK_SCORE");
        }
        _ => {}
    }
    set.into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_private_ip ----

    #[test]
    fn private_ipv4_ranges() {
        for ip in [
            "10.0.0.1",
            "10.255.255.255",
            "127.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.10.20",
        ] {
            assert!(is_private_ip(ip), "{ip} should be private");
        }
    }

    #[test]
    fn public_and_boundary_ipv4() {
        // 172 falls outside 16..=31 → public.
        assert!(!is_private_ip("172.15.0.1"));
        assert!(!is_private_ip("172.32.0.1"));
        // 192 outside .168 → public.
        assert!(!is_private_ip("192.167.1.1"));
        assert!(!is_private_ip("192.169.1.1"));
        assert!(!is_private_ip("8.8.8.8"));
        assert!(!is_private_ip("1.2.3.4"));
    }

    #[test]
    fn ipv6_loopback_and_ula() {
        assert!(is_private_ip("::1"));
        assert!(is_private_ip("[::1]:8080"));
        assert!(is_private_ip("fc00::1"));
        assert!(is_private_ip("fd12:3456::1"));
        assert!(!is_private_ip("2001:db8::1"));
    }

    #[test]
    fn garbage_is_not_private() {
        assert!(!is_private_ip(""));
        assert!(!is_private_ip("not-an-ip"));
        // Partial octets don't form a 4-piece address.
        assert!(!is_private_ip("10.0.0"));
    }

    // ---- risk_score ----

    #[test]
    fn score_clean_login_is_zero_or_clamped_low() {
        // Known user, device present, private ip (−10) → clamped to 0.
        assert_eq!(risk_score(false, 7, "admin", "10.0.0.1", true), 0);
        // Public ip, otherwise clean → 0.
        assert_eq!(risk_score(false, 7, "admin", "8.8.8.8", true), 0);
    }

    #[test]
    fn score_unknown_user_penalties() {
        // Anonymous (no uid, no username) weighs more than a known username.
        assert_eq!(risk_score(false, 0, "", "8.8.8.8", true), 20);
        assert_eq!(risk_score(false, 0, "someone", "8.8.8.8", true), 10);
        // uid ≠ 0 with a username carries no user penalty.
        assert_eq!(risk_score(false, 7, "admin", "8.8.8.8", true), 0);
    }

    #[test]
    fn score_device_ip_and_failure_terms() {
        assert_eq!(risk_score(false, 7, "admin", "8.8.8.8", false), 10);
        assert_eq!(risk_score(false, 7, "admin", "", true), 5);
        assert_eq!(risk_score(true, 7, "admin", "8.8.8.8", true), 50);
        // Failure + anonymous + no device + no ip.
        assert_eq!(risk_score(true, 0, "", "", false), 85);
    }

    // ---- risk_level ----

    #[test]
    fn level_boundaries() {
        assert_eq!(risk_level(0), "LOW");
        assert_eq!(risk_level(30), "LOW");
        assert_eq!(risk_level(31), "MEDIUM");
        assert_eq!(risk_level(70), "MEDIUM");
        assert_eq!(risk_level(71), "HIGH");
        assert_eq!(risk_level(100), "HIGH");
    }

    // ---- risk_factors ----

    #[test]
    fn factors_track_each_signal() {
        let factors = risk_factors(
            true,
            0,
            "",
            "",
            false,
            "FAILED",
            "incorrect password",
            "",
            85,
        );
        assert!(factors.contains(&"FAILED_LOGIN".to_string()));
        assert!(factors.contains(&"ANONYMOUS_LOGIN".to_string()));
        assert!(!factors.contains(&"UNKNOWN_USER".to_string()));
        assert!(factors.contains(&"UNKNOWN_DEVICE".to_string()));
        assert!(factors.contains(&"MFA_FAILED".to_string()));
        assert!(factors.contains(&"IP_MISSING".to_string()));
        assert!(!factors.contains(&"INTERNAL_IP".to_string()));
        assert!(factors.contains(&"PASSWORD_FAILURE".to_string()));
        assert!(factors.contains(&"NO_REQUEST_ID".to_string()));
        assert!(factors.contains(&"HIGH_RISK_SCORE".to_string()));
        // Always present.
        assert!(factors.contains(&"NO_SESSION".to_string()));
    }

    #[test]
    fn factors_internal_ip_mfa_unverified_and_band() {
        let factors = risk_factors(
            false,
            7,
            "admin",
            "192.168.1.1",
            true,
            "UNVERIFY",
            "mfa challenge expired",
            "req-1",
            50,
        );
        assert!(factors.contains(&"INTERNAL_IP".to_string()));
        assert!(!factors.contains(&"EXTERNAL_IP".to_string()));
        assert!(factors.contains(&"MFA_UNVERIFIED".to_string()));
        assert!(factors.contains(&"MFA_FAILURE_REASON".to_string()));
        assert!(factors.contains(&"MEDIUM_RISK_SCORE".to_string()));
        assert!(!factors.contains(&"NO_REQUEST_ID".to_string()));
    }

    #[test]
    fn factors_mfa_substring_is_literal_unverify() {
        // The probe is `contains("UNVERIFY")` over the uppercased status —
        // the English "UNVERIFIED" (…F-I-E-D) does not match, only a
        // literal UNVERIFY stem does.
        let factors = risk_factors(false, 7, "admin", "", true, "UNVERIFIED", "", "req", 0);
        assert!(!factors.contains(&"MFA_UNVERIFIED".to_string()));
        let factors = risk_factors(false, 7, "admin", "", true, "mfa_unverify", "", "req", 0);
        assert!(factors.contains(&"MFA_UNVERIFIED".to_string()));
    }

    #[test]
    fn factors_are_deduped_and_sorted() {
        let factors = risk_factors(false, 7, "admin", "8.8.8.8", true, "", "", "req", 0);
        let mut sorted = factors.clone();
        sorted.sort();
        assert_eq!(factors, sorted);
        let unique: std::collections::BTreeSet<_> = factors.iter().collect();
        assert_eq!(unique.len(), factors.len());
        // A clean login still records the external origin and no session.
        assert_eq!(factors, vec!["EXTERNAL_IP", "NO_SESSION"]);
    }
}
