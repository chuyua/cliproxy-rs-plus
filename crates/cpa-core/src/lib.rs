//! Contracts shared by every cliproxy-rs crate.
//!
//! - [`config`]: `config.yaml`, compatible with CLIProxyAPI.
//! - [`credential`]: runtime credentials loaded from `auth-dir` or config.
//! - [`exec`]: the execution envelope passed between the runtime and executors.
//! - [`format`]: request/response wire formats.
//! - [`gostr`]: Go's table-driven case and print rules (`strings.ToLower`, ...).
//! - [`registry`]: the pinned static model catalog.

pub mod config;
pub mod credential;
pub mod exec;
pub mod format;
pub mod gostr;
mod gostr_tables;
pub mod registry;
