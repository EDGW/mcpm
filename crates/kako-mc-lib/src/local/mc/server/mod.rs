//! Server identification from start scripts and authlib-injector agent identity.
//!
//! Registration metadata, configuration, launch plans and server snapshots are
//! mcpm's own data and live in `mcpm-backend::data`.

pub mod authlib_injector;
pub mod identify;
pub mod launch;
pub mod script_path;

pub use launch::{ServerLaunch, ServerOptionError};
pub use script_path::ServerScriptError;
