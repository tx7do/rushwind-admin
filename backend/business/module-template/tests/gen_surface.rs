//! The module's gen-surface guard tests: the route table, the audit
//! rows, the auth-free set, and the error table can only change by a
//! deliberate contract edit — these assertions turn silent drift into
//! a red test.

use module_template::gen::routes::ROUTES;
use module_template::{AUDIT_ROWS, AUTH_FREE};

/// The BFF surface is exactly the five notice routes — a new rpc
/// updates this count AND the AUDIT_ROWS literal below (the audit test
/// pins the pairing).
#[test]
fn route_table_is_the_five_notice_routes() {
    assert_eq!(ROUTES.len(), 5, "unexpected route count change");
    for spec in ROUTES {
        assert!(
            spec.path.starts_with("/template/v1/"),
            "route off the module prefix: {} {}",
            spec.method,
            spec.path
        );
        assert!(!spec.shadowed, "shadowed route emitted: {}", spec.path);
    }
}

/// The audit face rows must mirror the route table 1:1 — the audit
/// layer resolves operations against AUDIT_ROWS, so a drifted row
/// silently audits as `""` (operation audit skipped).
#[test]
fn audit_rows_mirror_the_route_table() {
    assert_eq!(AUDIT_ROWS.len(), ROUTES.len());
    for spec in ROUTES {
        assert!(
            AUDIT_ROWS
                .iter()
                .any(|(m, p, o)| *m == spec.method && *p == spec.path && *o == spec.operation_id),
            "audit row missing for {} {} ({})",
            spec.method,
            spec.path,
            spec.operation_id
        );
    }
}

/// The template ships fully gated: the whitelist is empty, so every
/// route must be classified gated by the generator.
#[test]
fn auth_free_set_is_empty_and_routes_all_gated() {
    assert!(AUTH_FREE.is_empty());
    for spec in ROUTES {
        let public = AUTH_FREE
            .iter()
            .any(|(s, m)| *s == spec.service_fq && *m == spec.method_name);
        assert!(
            !public,
            "route leaked to the public router: {} {}",
            spec.method, spec.path
        );
    }
}

/// The error table carries the module package: the reasons the
/// services raise resolve to their annotated HTTP statuses.
#[test]
fn error_table_resolves_the_module_reasons() {
    use module_template::tables::error_status;
    assert_eq!(error_status("notice.service.v1", "BAD_REQUEST"), Some(400));
    assert_eq!(error_status("notice.service.v1", "NOT_FOUND"), Some(404));
    assert_eq!(error_status("notice.service.v1", "CONFLICT"), Some(409));
    assert_eq!(error_status("notice.service.v1", "INTERNAL"), Some(500));
    // An unknown reason collapses to None (callers fold it into 500).
    assert_eq!(error_status("notice.service.v1", "NOPE"), None);
}

/// The annotated pool carries the module service — the bind layer
/// resolves request bindings against it.
#[test]
fn pool_carries_the_notice_service() {
    assert!(module_template::pool()
        .get_service_by_name("notice.service.v1.NoticeService")
        .is_some());
}
