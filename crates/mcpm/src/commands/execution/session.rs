//! Process executor construction and target-bound sessions for command dispatch.
//!
//! Commands never build providers or environments themselves: they receive a session bound
//! to their target, and local and remote dispatch use the same operation type.

use std::sync::Arc;

use anyhow::{Context, Result};
use mcpm_backend::{ResolvedWorkspace, Session, SurfaceEnv};
use mcpm_exec::{Failure, Settings, Target};

use crate::build_info;
use crate::ssh::SshArgs;

/// Builds a session for the target selected by global SSH options.
pub(crate) fn session_for(ssh: &SshArgs) -> Result<Session> {
    let target = if ssh.is_requested() {
        Target::Remote(ssh.connection()?)
    } else {
        Target::Local
    };
    build(target)
}

/// Builds a session for a resolved workspace, whose connection was already validated.
pub(crate) fn session_for_workspace(resolved: &ResolvedWorkspace) -> Result<Session> {
    let target = match &resolved.connection {
        Some(connection) => Target::Remote(connection.connection(default_executable())?),
        None => Target::Local,
    };
    build(target)
}

/// Builds a session that always runs locally, for commands fixed to the local target.
pub(crate) fn local_session() -> Result<Session> {
    build(Target::Local)
}

/// The remote executable used when a workspace does not override it.
pub(crate) fn default_executable() -> &'static str {
    "mcpmd"
}

fn build(target: Target) -> Result<Session> {
    let surface = SurfaceEnv::new().context("failed to initialize command environment")?;
    let settings = Settings::new(
        build_info::VERSION,
        crate::logging::level().into(),
        Arc::new(crate::logging::replay),
    );
    Ok(Session::new(mcpm_backend::executor(surface, settings), target))
}

/// Gracefully closes a remote session so the remote process can acknowledge shutdown.
pub(crate) fn finish(
    runtime: &crate::commands::execution::Runtime,
    session: &Session,
) -> Result<()> {
    if !session.target().is_remote() {
        return Ok(());
    }
    runtime
        .block_on(session.close())
        .map_err(anyhow::Error::new)
}

/// Renders a surface-operation failure, which keeps its own error chain.
pub(crate) fn surface_failure<E>(failure: Failure<E>) -> anyhow::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    match failure {
        Failure::Operation(error) => anyhow::Error::new(error),
        Failure::Dispatch(error) => anyhow::Error::new(error),
        Failure::Transport(error) => anyhow::Error::new(error),
        Failure::Cancelled => anyhow::anyhow!("command was cancelled"),
    }
}

/// Renders one operation failure, preserving the remote origin for the display layer.
pub(crate) fn failure<K>(
    failure: Failure<mcpm_backend::error::OperationError<K>>,
    remote: bool,
) -> anyhow::Error
where
    K: std::fmt::Debug + Send + Sync + 'static,
{
    match failure {
        Failure::Operation(error) => {
            let error = error.into_error();
            if remote {
                crate::diagnostics::RemoteError::wrap(error)
            } else {
                error
            }
        }
        Failure::Dispatch(error) => anyhow::Error::new(error),
        Failure::Transport(error) => anyhow::Error::new(error),
        Failure::Cancelled => anyhow::anyhow!("command was cancelled"),
    }
}
