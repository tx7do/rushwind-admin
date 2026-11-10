//! Handwritten table accessors over the generated annotation tables.
//!
//! Sits beside the generated surface it indexes (the framework's
//! http-binding crate stays deployment-free); consumers like the auth gate
//! and the service state resolve reason → status through here.

/// Resolves a (package, reason) pair against the generated `(errors.code)`
/// annotation table. Callers must collapse `None` into the Unknown error
/// shape (500, empty reason) — via `StatusError::new(500, "", ..)`.
pub fn error_status(package: &str, reason: &str) -> Option<i32> {
    crate::proto::gen::error_tables::ERROR_STATUS
        .iter()
        .find(|(p, r, _)| *p == package && *r == reason)
        .map(|(_, _, s)| *s)
}
