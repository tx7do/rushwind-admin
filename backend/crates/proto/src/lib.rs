//! The admin API contract crate — everything generated from the synced
//! proto tree (`api/protos`, checksummed by `api/MANIFEST.sha256`):
//!
//! * [`proto`] — the types: prost structs + pbjson protojson serde.
//! * [`DESCRIPTOR_BYTES`] / [`pool`] — the annotated full compile closure:
//!   buf-produced (protox's serializer would drop the custom-option
//!   bytes — google.api.http / errors.code — which are the point), decoded
//!   once into the immutable process-global pool. The schema surface for
//!   protojson serialization and form binding, per `docs/binding-spec.md`.
//! * [`gen`] — the build-time-emitted surface (the framework's
//!   `rushwind-gen-http` over the annotated closure): route table with
//!   form-binding plans, reason → HTTP status error tables, one service
//!   trait per annotated BFF service, null placeholder impls, and the public/gated mount
//!   emitters split by [`AUTH_FREE`].
//! * [`tables`] — handwritten accessors over the generated error tables.
//!
//! Wire semantics are pinned by `docs/binding-spec.md`.

use std::sync::OnceLock;

use prost_reflect::DescriptorPool;

/// Generated prost types + pbjson serde impls for the admin contract modules.
///
/// Generated code carries no doc comments and is not held to hand-written lint
/// standards; the contract prose lives in the .proto sources.
#[allow(missing_docs)]
#[allow(clippy::all)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/proto_include.rs"));
}

/// The admin proto compile closure as raw `FileDescriptorSet` bytes —
/// annotations included — compiled by the build script from the checksummed
/// contract tree and its vendored annotation declarations
/// (`backend/api/third_party`).
pub static DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/annotated_descriptor.bin"));

/// The decoded descriptor pool. The pool is immutable and process-global;
/// decoding happens once.
pub fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(DESCRIPTOR_BYTES).expect("annotated_descriptor.bin must decode")
    })
}

/// Generated route/error/trait/mount surface — do not edit; regenerate by
/// building.
#[allow(missing_docs)]
#[allow(clippy::all)]
pub mod gen {
    include!(concat!(env!("OUT_DIR"), "/admin_gen.rs"));
}

/// Handwritten table accessors over the generated annotation tables.
pub mod tables;

include!("auth_free.rs");
