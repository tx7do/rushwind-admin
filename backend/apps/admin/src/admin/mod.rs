//! The synced admin surface — everything an upstream sync wave lands
//! in: the 41 service implementations, the SeaORM data layer, the
//! route-pack edge and its docs/audit/SSE/task transports, the
//! migration and seed chains, and the runtime state. A fork reads this
//! subtree; a fork never edits it (fork code lives in
//! [`crate::admin::modules`] beside the assembly lines in `main.rs`).

pub mod assets;
pub mod audit;
pub mod authorizer;
pub mod captcha;
pub mod config;
pub mod crypto;
pub mod data;
pub(crate) mod macros;
pub(crate) mod mapping;
pub mod migration;
pub mod paging;
pub mod policy;
pub mod ratelimit;
pub mod seed;
pub mod server;
pub mod services;
pub mod state;
pub mod token;
