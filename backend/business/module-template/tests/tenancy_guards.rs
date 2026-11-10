//! The tenancy guard: the delete path must resolve its row through the
//! viewer-scoped repo, never by bare id. A bare-id delete crosses
//! tenants — any authenticated caller could remove another tenant's
//! row by guessing ids. This is a source-text guard because the
//! failure is a *shape* (the predicate silently dropped in a rewrite),
//! not a behavior a unit test without a database can catch.

use module_template as _;

/// The delete implementation must go through the tenanted fetch and
/// 404 off-scope rows (`repo.get` carries the viewer predicate, the
/// shell deliberately leaves tenant-scoped get/delete hand-written).
#[test]
fn delete_resolves_through_the_tenanted_repo() {
    let src = include_str!("../src/services.rs");
    let del = src
        .split("async fn delete")
        .nth(1)
        .expect("the delete handler must exist");
    let body = &del[..del.find("async fn").unwrap_or(del.len())];
    // rustfmt may wrap `repo.get(` across lines — match on the
    // whitespace-stripped text.
    let flat: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("repo.get("),
        "delete must fetch through the tenanted repo (viewer predicate), got:\n{body}"
    );
    assert!(
        flat.contains("not_found"),
        "delete must answer 404 for off-scope rows, got:\n{body}"
    );
}

/// The repo's own get must carry the viewer predicate — the one
/// hand-written seam the shell leaves beside the invocation.
#[test]
fn repo_get_carries_the_viewer_predicate() {
    let src = include_str!("../src/data.rs");
    let get = src
        .split("pub async fn get")
        .nth(1)
        .expect("the tenant-scoped get must exist");
    let body = &get[..get.find("}").unwrap_or(get.len())];
    assert!(
        body.contains(".filter(self.condition())"),
        "repo get must filter by the viewer predicate, got:\n{body}"
    );
}

/// Compile-time pin: the repo type stays a tenant shell.
#[test]
fn notice_repo_is_the_tenant_shell() {
    // `tenant` arm derives the predicate from the Viewer; the `global`
    // arm has none and would silently unscope every handler.
    let src = include_str!("../src/data.rs");
    assert!(
        src.contains("repo_shell!(tenant NoticeRepo, notices)"),
        "NoticeRepo must ride the tenant shell arm"
    );
}
