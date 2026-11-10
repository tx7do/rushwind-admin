//! The transport servers: the REST router with its per-route layer
//! composition, the SSE push hub, the task-queue worker + cron producer,
//! and the API-docs endpoints.

pub mod apalis;
pub mod docs;
pub mod edge;
pub mod rest;
pub mod sse;

/// The settings-node parse the server factory wires share: a missing
/// node leaves the type's default, a malformed one fails the bootstrap
/// with the caller's prefix.
pub fn settings_or_default<T>(
    settings: serde_json::Value,
    prefix: &str,
) -> Result<T, rushwind_bootstrap::BootstrapError>
where
    T: serde::de::DeserializeOwned + Default,
{
    if settings.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(settings)
        .map_err(|e| rushwind_bootstrap::BootstrapError::Config(format!("{prefix}: {e}")))
}
