//! Effective data source of a server command, resolved by the backend workspace operation.
//!
//! Commands only carry CLI options; whether they act on the calling machine or a registered
//! Remote workspace is decided by `workspace.resolve` and returned as a connection plus an
//! optional registration view.

use std::path::PathBuf;

use anyhow::Result;
use mcpm_backend::workspace::{Resolve, ResolveInput};
use mcpm_backend::{ConnectionSpec, RemoteView, ResolvedWorkspace};

use crate::commands::execution::{default_executable, session_for_workspace};
use crate::ssh::SshArgs;

/// Effective data source plus the local registration used to select it, if any.
pub(super) struct Target {
    pub path: PathBuf,
    pub remote: Option<RemoteView>,
    /// Session bound to the resolved data source; every migrated operation uses it.
    pub session: mcpm_backend::Session,
}

pub(super) fn resolve(path: PathBuf, ssh: SshArgs) -> Result<Target> {
    ssh.validate()?;
    let explicit = if ssh.is_requested() {
        let (command, executable) = ssh.settings()?;
        Some(ConnectionSpec {
            ssh: command,
            executable,
            directory: ssh.directory().cloned(),
        })
    } else {
        None
    };
    let resolved = run(ResolveInput {
        path,
        explicit,
        default_executable: default_executable().to_owned(),
    })?;
    let session = session_for_workspace(&resolved)?;
    Ok(Target {
        path: resolved.path.clone(),
        remote: resolved.remote.clone(),
        session,
    })
}

/// Resolves the workspace through the backend surface operation.
pub(super) fn run(input: ResolveInput) -> Result<ResolvedWorkspace> {
    let runtime = crate::commands::execution::Runtime::new()?;
    let session = crate::commands::execution::local_session()?;
    runtime.block_on(async {
        let handle = session
            .start::<Resolve>(input)
            .await
            .map_err(|failure| session_failure(failure, false))?;
        handle
            .finish()
            .await
            .map_err(|failure| session_failure(failure, false))
    })
}

/// Rejects a Remote workspace until the command has its own operation.
pub(super) fn require_supported_local(path: &std::path::Path, command: &str) -> Result<()> {
    mcpm_backend::workspace::require_local(path, command)
}


fn session_failure(
    failure: mcpm_exec::Failure<mcpm_backend::workspace::ResolveError>,
    remote: bool,
) -> anyhow::Error {
    match failure {
        mcpm_exec::Failure::Operation(error) => {
            let error = anyhow::Error::new(error);
            if remote {
                crate::diagnostics::RemoteError::wrap(error)
            } else {
                error
            }
        }
        mcpm_exec::Failure::Dispatch(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Transport(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Cancelled => anyhow::anyhow!("workspace resolution was cancelled"),
    }
}
