//! The admin application — one crate, one binary, the whole product.
//!
//! Three rooms, one ownership line:
//!
//! * [`admin`] — the synced admin surface (upstream sync waves land
//!   here and in the `api/` admin contract subtree; a fork reads it,
//!   never edits it);
//! * [`proto`] — the contract face: one buf workspace compiling the
//!   admin tree and the fork's module trees into one descriptor pool,
//!   one set of types, and the per-face gen surface;
//! * [`modules`] — the fork's own business modules (copy-rename the
//!   `template` module; everything a fork writes lives here beside the
//!   assembly lines in `main.rs`).
//!
//! The build script (biz-proto-build over `api/`) emits the types, the
//! annotated closure, and one gen face per module tree; the pool and
//! the redaction plan resolve at the crate root because the generated
//! mounts' knob expressions say so (`crate::pool()` /
//! `crate::redact_plan()`).

pub mod admin;
pub mod modules;
pub mod proto;

include!("auth_free.rs");

// The assembly faces `main.rs` composes, re-exported at the root so
// the entry reads as one flat surface (and `repo_shell!`'s `$crate`
// targets resolve: the macro bodies name `crate::proto`, `crate::data`,
// `crate::state`, `crate::paging`).
pub use admin::{authorizer, config, crypto, data, migration, paging, seed, server, state};

// The field-level update gate and its test fixture, root-re-exported so
// the (synced) handler and test bodies keep naming `crate::fieldmask`
// and `crate::testsupport` verbatim.
pub(crate) use admin::fieldmask;
#[cfg(test)]
pub(crate) use admin::testsupport;

use std::sync::OnceLock;

use prost_reflect::DescriptorPool;

/// The proto compile closure as raw `FileDescriptorSet` bytes —
/// annotations included — compiled by the build script from the
/// checksummed contract tree and the buf.lock-pinned dependency
/// modules (`api/buf.yaml`).
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

/// The static redaction plan over the annotated pool: every `(redact.v1)`
/// option the contract tree carries (user email/mobile masks, the
/// `element = { nested: true }` list envelopes, the i_user BFF's
/// `method_skip` set), fail-closed resolved once per process — an
/// unsupported option shape refuses the build, exactly like the plan's
/// source annotations refuse silent degradation. The generated mounts
/// thread this to the lifecycle glue, so every response redacts before
/// it serializes; the reference's redacted server wrappers are the
/// behavior anchor.
pub fn redact_plan() -> &'static rushwind_redact::RedactPlan {
    static PLAN: OnceLock<rushwind_redact::RedactPlan> = OnceLock::new();
    PLAN.get_or_init(|| {
        rushwind_redact::RedactPlan::build(pool())
            .expect("redact plan over the annotated contract pool must build")
    })
}
