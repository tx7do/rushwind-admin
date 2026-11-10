//! The template module's gen-surface guard tests: the route table, the
//! auth-free set, and the error table can only change by a deliberate
//! contract edit — these assertions turn silent drift into a red test.
//! (The audit face derives from this same ROUTES table by construction
//! — the old hand-copied AUDIT_ROWS literal and its mirror guard died
//! with the single workspace.)

use admin::proto::gen::template::routes::ROUTES;

/// The BFF surface is exactly the five notice routes — a new rpc
/// updates this count.
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

/// The module ships fully gated: no notice route may appear in the
/// deployment whitelist (the admin face's own entries are its
/// business), so every module route must be classified gated by the
/// generator.
#[test]
fn auth_free_carries_no_module_routes_and_routes_all_gated() {
    use admin::AUTH_FREE;
    for (svc, _) in AUTH_FREE {
        assert!(
            !svc.starts_with("notice.service.v1"),
            "module route leaked into the deployment whitelist: {svc}"
        );
    }
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
    use admin::proto::gen::template::error_tables::ERROR_STATUS;
    let resolve = |reason: &str| {
        ERROR_STATUS
            .iter()
            .find(|(p, r, _)| *p == "notice.service.v1" && *r == reason)
            .map(|(_, _, s)| *s)
    };
    assert_eq!(resolve("BAD_REQUEST"), Some(400));
    assert_eq!(resolve("NOT_FOUND"), Some(404));
    assert_eq!(resolve("CONFLICT"), Some(409));
    assert_eq!(resolve("INTERNAL"), Some(500));
    // An unknown reason collapses to None (callers fold it into 500).
    assert_eq!(resolve("NOPE"), None);
}

/// The annotated pool carries the module service — the bind layer
/// resolves request bindings against it. The pool is the ONE
/// process-global closure over the whole workspace: the admin services
/// and the module services decode from the same bytes.
#[test]
fn pool_carries_the_notice_service() {
    assert!(admin::pool()
        .get_service_by_name("notice.service.v1.NoticeService")
        .is_some());
    assert!(
        admin::pool()
            .get_service_by_name("admin.service.v1.UserService")
            .is_some(),
        "the module tree must not have evicted the admin tree from the pool"
    );
}
