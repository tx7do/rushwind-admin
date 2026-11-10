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
