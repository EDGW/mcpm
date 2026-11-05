//! Resolve a Mod flow's execution target without interpreting ordinary Mod paths as workspaces.

use crate::ssh::SshArgs;
use anyhow::{Context, Result};
use mcpm_backend::operations::mods::loading::ScopeInput;
use mcpm_exec::service::Executor;

pub(in crate::commands) async fn executor(
    scope: &mut ScopeInput,
    ssh: SshArgs,
) -> Result<Executor> {
    let path = match scope {
        ScopeInput::Server { path, .. } => Some(std::mem::take(path)),
        ScopeInput::Directory { .. } => None,
    };
    let (executor, resolved, _) = ssh.create_executor(path).await?;
    if let ScopeInput::Server { path, .. } = scope {
        *path = resolved.context("missing resolved Server Mod path")?;
    }
    Ok(executor)
}
