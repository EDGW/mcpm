//! Workspace operations: selecting a data source and recording a Remote workspace.
//!
//! Both are surface operations: they act on the machine presenting the workspace and
//! never connect to the host they select.

pub mod git;
pub mod register;
pub mod resolve;
pub mod types;
pub mod validate;

use std::path::Path;

pub use register::{RegisterError, RegisterInput, RegisterOutput, RemoteRegister};
pub use resolve::{Resolve, ResolveError, ResolveInput};
pub use types::{ConnectionSpec, RemoteView, ResolvedWorkspace};
pub use validate::{Validate, ValidateError, ValidateInput, ValidateOutput};

/// Rejects a Remote workspace for a command that only supports Local Servers.
///
/// Temporary until every server command runs through an operation that reports its own
/// classification.
pub fn require_local(path: &Path, command: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::data::workspace::kind(path)? == crate::data::workspace::Kind::Local,
        "{command} does not support Remote workspaces"
    );
    Ok(())
}
