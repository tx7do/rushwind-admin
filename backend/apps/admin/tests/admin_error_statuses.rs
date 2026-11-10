//! The gate-status equivalence guard: the auth gate (the framework
//! crate) pins its failure statuses as literals (401/403 — the
//! differential harness byte-compares those envelopes), and the admin
//! contract's error table anchors the same pair to `UNAUTHORIZED` /
//! `FORBIDDEN`. This test keeps the two declarations equivalent, so a
//! contract edit that moves the anchor shows up here instead of
//! silently drifting the gate's envelope.

use admin::proto::tables::error_status;

#[test]
fn gate_statuses_match_the_admin_error_table() {
    assert_eq!(
        error_status("admin.service.v1", "UNAUTHORIZED"),
        Some(401),
        "the table moved UNAUTHORIZED — the auth gate's literal needs the same edit"
    );
    assert_eq!(
        error_status("admin.service.v1", "FORBIDDEN"),
        Some(403),
        "the table moved FORBIDDEN — the auth gate's literal needs the same edit"
    );
}
