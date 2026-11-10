//! Service-layer boilerplate macros shared across the per-service
//! files — and exported for the business modules: a downstream module
//! crate implements its services against the same helpers (see
//! `business/module-template` for the consumer face).

/// The Id-only `query_by` extraction: returns the id, or fails with the
/// shared `query_by required` rejection for every other shape. The path
/// is the request's nested `QueryBy` enum.
#[macro_export]
macro_rules! query_by_id {
    ($query_by:expr, $($seg:ident)::+) => {
        match $query_by {
            Some($($seg)::+::Id(id)) => id,
            _ => return Err($crate::state::status_error("BAD_REQUEST", "query_by required")),
        }
    };
}

/// Stamp the editor columns on an update path: `updated_by` moves to
/// the operator and `updated_at` to now — always together.
#[macro_export]
macro_rules! stamp_update {
    ($model:expr, $user:expr) => {
        $model.updated_by = sea_orm::Set(Some($user));
        $model.updated_at = sea_orm::Set(Some($crate::data::now()));
    };
}
