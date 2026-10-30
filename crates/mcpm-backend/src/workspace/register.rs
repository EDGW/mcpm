//! `workspace.remote.register`: records caller-side connection data for a Remote placeholder.
//!
//! This surface operation writes only the caller workspace marker and
//! `.mcpm/remote_info.json`. It does not inspect or modify host server business data and never
//! performs Git setup; host initialization and Git belong to `server.register` on the host.

use std::{fs, future::Future, path::PathBuf};

use anyhow::{Context as _, Result};
use mcpm_exec::{Context, Operation, Placement};
use serde::{Deserialize, Serialize};

use crate::data::workspace::{RegistrationWriter, RemoteRegistration};
use crate::env::SurfaceEnv;

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
    /// Replace an existing caller-side connection registration.
    #[serde(default)]
    pub replace: bool,
}

/// What the registration wrote.
#[derive(Debug)]
pub struct RegisterOutput {
    pub remote: RemoteView,
}

fn register(input: RegisterInput) -> Result<RegisterOutput> {
    let directory = fs::canonicalize(&input.path)
        .context("Remote workspace PATH must be an existing local directory")?;
    let default_executable = "mcpm-tools".to_owned();
    let executable = input.connection.executable.clone().unwrap_or_else(|| default_executable.clone());
    input.connection.connection(&default_executable)
        .map_err(|error| anyhow::anyhow!("invalid SSH settings: {error}"))?;
    let registration = RemoteRegistration::new(
        input.connection.ssh.clone(),
        Some(executable.clone()),
        input.remote_path.clone(),
    )?;
    let view = RemoteView {
        path: input.remote_path,
        ssh: input.connection.ssh.clone(),
        destination: mcpm_exec::ssh::Connection::new(input.connection.ssh, executable.clone()).destination(),
        executable,
    };
    tracing::info!(workspace = %input.path.display(), "Registering Remote workspace");
    let writer = RegistrationWriter::acquire(&directory)?;
    writer.remote(&registration, input.replace)?;
    Ok(RegisterOutput { remote: view })
}

/// A Remote registration could not be written.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct RegisterError(#[from] pub anyhow::Error);
