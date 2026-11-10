//! The fork's business modules — everything a fork owns. A module is a
//! directory pair: `src/modules/<name>/` (this side: the entity, the
//! repo, the service implementation, the idempotent seeds, the
//! migration chain, and the route pack) and `api/fork/protos/<name>/`
//! (the contract side, compiled into the shared workspace beside the
//! admin tree). Adding a module is a copy-rename of [`template`] plus
//! the four assembly lines in `main.rs` — nothing outside this tree
//! and those lines ever changes.
pub mod template;
