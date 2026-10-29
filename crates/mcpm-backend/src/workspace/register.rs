//! `workspace.remote.register`: records an existing remote Local Server in a local workspace.
//!
//! The registration file is always written on the machine presenting the workspace. The
//! remote server itself is validated by the caller through `server.info` before this
//! operation runs.

use std::{fs, future::Future, path::PathBuf};

use anyhow::{Context as _, Result};
use mcpm_exec::{Context, Operation, Placement};
use serde::{Deserialize, Serialize};

use crate::data::workspace::{RegistrationWriter, RemoteRegistration};
use crate::env::SurfaceEnv;

use super::git;
use super::types::{ConnectionSpec, RemoteView};

/// Registers one Remote workspace.
#[derive(Debug)]
pub struct RemoteRegister;

crate::surface_operation!(RemoteRegister, "workspace.remote.register");

impl Operation for RemoteRegister {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = RegisterInput;
    type Event = ();
    type Output = RegisterOutput;
    type Error = RegisterError;

    fn run(
        input: RegisterInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || register(input)).await.map_err(RegisterError)
        }
    }
}

/// One Remote registration request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    /// Existing local workspace directory.
    pub path: PathBuf,
    pub connection: ConnectionSpec,
    pub remote_path: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub no_git: bool,
    #[serde(default)]
    pub git: bool,
    /// Remote executable used when the connection does not name one.
    pub default_executable: String,
}

/// What the registration wrote and which Git setup ran.
#[derive(Debug)]
pub struct RegisterOutput {
    pub remote: RemoteView,
    pub git: Option<git::Outcome>,
}

fn register(input: RegisterInput) -> Result<RegisterOutput> {
    git::validate_options(input.git, input.no_git)?;
    let directory = fs::canonicalize(&input.path)
        .context("Remote workspace PATH must be an existing local directory")?;
    let executable = input
        .connection
        .executable
        .clone()
        .unwrap_or_else(|| input.default_executable.clone());
    input
        .connection
        .connection(&input.default_executable)
        .map_err(|error| anyhow::anyhow!("invalid SSH settings: {error}"))?;
    let registration = RemoteRegistration::new(
        input.connection.ssh.clone(),
        Some(executable.clone()),
        input.remote_path.clone(),
    )?;
    let view = RemoteView {
        path: input.remote_path,
        ssh: input.connection.ssh.clone(),
        destination: mcpm_exec::ssh::Connection::new(input.connection.ssh, executable.clone())
            .destination(),
        executable,
    };
    tracing::info!(workspace = %input.path.display(), "Registering Remote workspace");
    let writer = RegistrationWriter::acquire(&directory)?;
    let git = if input.no_git {
        None
    } else {
        Some(git::ensure_repository(&directory, input.git)?)
    };
    writer.remote(&registration, input.force)?;
    Ok(RegisterOutput { remote: view, git })
}

/// A Remote registration could not be written.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct RegisterError(#[from] pub anyhow::Error);
