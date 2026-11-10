//! The contract face — everything generated from the api/ workspace
//! (the admin contract tree synced from upstream + the fork's module
//! trees): prost types + pbjson protojson serde (one workspace, one
//! compile, one type per proto package — the paging envelope exists
//! exactly once), the buf-produced annotated closure, and the
//! per-face gen surface (route/binding/trait/mount, one face per
//! source tree). The pool and the redaction plan live at the crate
//! root (`crate::pool()` / `crate::redact_plan()` — the engine's knob
//! expressions).
//!
//! Types resolve as `crate::proto::<package segments>` — the
//! generator's `proto_module_path` knob names this module. Generated
//! code carries no doc comments and is not held to hand-written lint
//! standards; the contract prose lives in the .proto sources.
#![allow(missing_docs)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/proto_include.rs"));

/// The proto compile closure as raw `FileDescriptorSet` bytes —
/// annotations included — compiled by the build script from the
/// checksummed admin contract tree, the fork's module trees, and the
/// buf.lock-pinned dependency modules (`api/buf.yaml`).
pub static DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/annotated_descriptor.bin"));

/// The generated route/error/trait/mount surfaces — do not edit;
/// regenerate by building. One module per face: `gen` is the admin
/// face (the synced registration set), `gen::template` the first fork
/// face (a copy-renamed module adds its own sibling).
pub mod gen {
    include!(concat!(env!("OUT_DIR"), "/admin_gen.rs"));

    /// The template module's face.
    pub mod template {
        include!(concat!(env!("OUT_DIR"), "/template_gen.rs"));
    }
}

/// Handwritten table accessors over the generated annotation tables.
pub mod tables;

/// The auth-free whitelist is defined at the crate root
/// (`include!("auth_free.rs")` in lib.rs) and re-exported here because
/// the contract's consumers (the corpus guard, the differential
/// harness) reach for it through the contract face.
pub use crate::AUTH_FREE;

/// The generated `PagingRequest` resolves into the framework paging
/// pipeline's params — the impl lives in THIS crate because the orphan
/// rule pins it to the type's owner; the repositories' call sites keep
/// passing the request verbatim.
impl rushwind_storage_seaorm_support::paging::PagingInput
    for crate::proto::pagination::PagingRequest
{
    fn paging_params(&self) -> rushwind_storage_seaorm_support::paging::Params {
        use crate::proto::pagination::paging_request::FilteringType;
        rushwind_storage_seaorm_support::paging::Params {
            query: match &self.filtering_type {
                Some(FilteringType::Query(query)) => Some(query.clone()),
                _ => None,
            },
            order_by: self.order_by.clone(),
            sorting: self
                .sorting
                .iter()
                .filter(|s| !s.field.is_empty())
                .map(|s| rushwind_storage_seaorm_support::paging::Sorting {
                    field: s.field.clone(),
                    desc: s.direction == 1, // Direction::DESC
                })
                .collect(),
            page: self.page,
            page_size: self.page_size,
            offset: self.offset,
            limit: self.limit,
            no_paging: self.no_paging.unwrap_or(false),
        }
    }
}
