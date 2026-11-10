//! The copy-rename template module: a tenant-scoped notice domain
//! walking the full chain — contract (`api/fork/protos/notice`,
//! compiled into the shared workspace) → service implementation over
//! the generated trait (`services`), the repo shell (`data`), the
//! idempotent boot seeds (menu / permission / sys_apis), and the
//! module's own migration chain — mounted beside the admin surface by
//! the assembly lines in `main.rs`.
//!
//! Copy-rename checklist: move `src/modules/template/` and
//! `api/fork/protos/notice/` together to the new names (proto package,
//! directories, and import paths move as one), update `mod.rs`'s
//! registrations, then add the four lines in `main.rs`. Nothing else
//! in the app knows this module exists.

use std::sync::Arc;

/// The module's audit face: the route rows derive straight from the
/// face's generated ROUTES table — the same 1:1 the admin face rides,
/// with no hand-copied literal to drift (the old `AUDIT_ROWS`
/// const-and-guard pair died with the single workspace: the rows are
/// the gen table by construction now).
fn audit_face() -> Arc<crate::admin::audit::AuditFace> {
    let rows: Vec<(&'static str, &'static str, &'static str)> = crate::proto::gen::template::routes::ROUTES
        .iter()
        .map(|spec| (spec.method, spec.path, spec.operation_id))
        .collect();
    Arc::new(crate::admin::audit::AuditFace::from_rows(&rows))
}

/// The module's error envelope helper: reason → the module error
/// table's HTTP status (`StatusError::new`) and the `<Reason>` literal
/// (the admin state helper's mirror over the module's own face table).
pub fn status_error(
    reason: &'static str,
    message: impl Into<String>,
) -> crate::admin::state::StatusError {
    let status = crate::proto::gen::template::error_tables::ERROR_STATUS
        .iter()
        .find(|(p, r, _)| *p == "notice.service.v1" && *r == reason)
        .map(|(_, _, s)| *s)
        .unwrap_or(500);
    rushwind_http_binding::envelope::StatusError::new(status, reason, message)
}

/// The module's not-found error (404 + `NOT_FOUND`).
pub fn not_found(what: &str) -> crate::admin::state::StatusError {
    status_error("NOT_FOUND", format!("{what} not found"))
}

/// The deployment's registered codec subtypes — the same register the
/// admin surface mounts under (the compatibility spec §2.1 set).
const REGISTERED_SUBTYPES: &[&str] = &["json", "proto", "x-www-form-urlencoded"];

pub mod data;
pub mod migration;
pub mod seed;
pub mod services;

/// The module's route pack over the admin edge: the same per-route
/// stack (bind outermost, auth gate inside), the same audit layer, no
/// docs face (the module ships no OpenAPI asset). Mounted beside the
/// admin surface by the assembly binary entry (`main.rs`).
pub fn pack(
    state: Arc<crate::admin::state::AppState>,
) -> impl Fn(
    serde_json::Value,
    rushwind_bootstrap::RouteInput,
) -> Result<rushwind_bootstrap::RouteSurface, rushwind_bootstrap::BootstrapError>
       + Send
       + Sync
       + 'static {
    crate::admin::server::edge::pack_with(move |_settings| {
        Ok(crate::admin::server::edge::edge_router(
            &state,
            crate::admin::server::edge::EdgePack {
                pool: crate::pool(),
                subtypes: REGISTERED_SUBTYPES,
                mounts: vec![Box::new({
                    let state = Arc::clone(&state);
                    move |router_pub: axum::Router,
                          router_gate: axum::Router,
                          wrap: &crate::admin::server::edge::Wrap|
                          -> (axum::Router, axum::Router) {
                        crate::proto::gen::template::mounts::mount_notice_service(
                            router_pub,
                            router_gate,
                            Arc::new(crate::modules::template::services::NoticeService {
                                state: Arc::clone(&state),
                            }),
                            wrap,
                        )
                    }
                })],
                docs: None,
                audit: Some(audit_face()),
                extras: vec![],
            },
        ))
    })
}
