//! Corpus guard for the auth-free split.
//!
//! The generator classifies every route binding via the auth-free table
//! (`proto::AUTH_FREE`, the deployment's auth-free set). This test pins
//! BOTH directions against the live corpus:
//!
//! * every whitelisted pair still exists as a route binding — a dropped
//!   proto method would silently widen the gated set (a parity break);
//! * nothing beyond the whitelist classifies public — a table typo would
//!   silently expose a protected endpoint (a security break);
//! * the public binding count matches, so `additional_bindings` of a
//!   whitelisted operation stay exempt with their primary binding.

use std::collections::BTreeSet;

/// The expected public pairs — the deployment's public whitelist
/// registrations, verbatim.
const EXPECTED: &[(&str, &str)] = &[
    ("admin.service.v1.AuthenticationService", "Login"),
    ("admin.service.v1.AuthenticationService", "GenerateCaptcha"),
    ("admin.service.v1.AuthenticationService", "VerifyCaptcha"),
    ("admin.service.v1.AuthenticationService", "RefreshToken"),
    ("admin.service.v1.MfaService", "VerifyMFAChallenge"),
    ("admin.service.v1.AccessKeyService", "IssueToken"),
    ("admin.service.v1.AuthenticationService", "ForgotPassword"),
    (
        "admin.service.v1.AuthenticationService",
        "ResetPasswordByCode",
    ),
];

#[test]
fn auth_free_routes_match_the_go_whitelist() {
    let mut public: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut public_bindings = 0usize;
    for route in proto::gen::routes::ROUTES {
        if proto::AUTH_FREE
            .iter()
            .any(|(s, m)| *s == route.service_fq && *m == route.method_name)
        {
            public.insert((route.service_fq, route.method_name));
            public_bindings += 1;
        }
    }
    let expected: BTreeSet<(&str, &str)> = EXPECTED.iter().copied().collect();

    assert_eq!(
        public, expected,
        "the public route set diverged from the auth-free set"
    );
    assert_eq!(
        public_bindings,
        EXPECTED.len(),
        "the public binding count diverged (additional_bindings drift)"
    );
}

/// The shadow set: bindings a first-match mux absorbs
/// never reach (an earlier same-method pattern route absorbs their
/// paths), so the mounts skip them. Pinned hard — contract drift that
/// adds or removes a shadow must land here consciously.
#[test]
fn shadowed_routes_match_the_registered_set() {
    let observed: Vec<(usize, &str, &str)> = proto::gen::routes::ROUTES
        .iter()
        .enumerate()
        .filter(|(_, r)| r.shadowed)
        .map(|(i, r)| (i, r.method, r.path))
        .collect();
    let expected: Vec<(usize, &str, &str)> = vec![
        // i_api.proto: /admin/v1/apis/{id} (GET, registered earlier in the
        // same file) absorbs the walk-route literal path. The index moved
        // 16 → 52 with the ce8ab49 vintage acceptance (the AI/BI/print/
        // monitor-alert BFF services register ahead of i_api.proto).
        (52, "GET", "/admin/v1/apis/walk-route"),
    ];
    assert_eq!(
        observed, expected,
        "the gorilla-shadow set diverged (registration-order or path-shape drift)"
    );
}
