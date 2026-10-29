//! `workspace.resolve`: selects the data source of a workspace command.
//!
//! The operation only reads the local workspace marker. It never connects: a Remote
//! registration becomes a connection specification that the caller's session opens
//! lazily, and an explicit connection bypasses local inspection entirely.

use std::{fs, future::Future, path::PathBuf};

use anyhow::Context as _;
use mcpm_exec::{Context, Operation, Placement};
use serde::{Deserialize, Serialize};

use crate::data::workspace::{Kind, RemoteRegistration, kind};
use crate::env::SurfaceEnv;

use super::types::{ConnectionSpec, RemoteView, ResolvedWorkspace};

/// Resolves the effective data source of one workspace path.
#[derive(Debug)]
pub struct Resolve;

crate::surface_operation!(Resolve, "workspace.resolve");

impl Operation for Resolve {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = ResolveInput;
    type Event = ();
    type Output = ResolvedWorkspace;
    type Error = ResolveError;

    fn run(
        input: ResolveInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || resolve(input)).await.map_err(ResolveError)
        }
    }
}

/// One resolution request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    pub path: PathBuf,
    /// An explicit `--ssh` selection; it takes precedence over the workspace marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit: Option<ConnectionSpec>,
    /// Remote executable used when neither the caller nor the registration names one.
    pub default_executable: String,
}

fn resolve(input: ResolveInput) -> Result<ResolvedWorkspace, anyhow::Error> {
    if let Some(connection) = input.explicit {
        connection
            .connection(&input.default_executable)
            .map_err(|error| anyhow::anyhow!("invalid SSH settings: {error}"))?;
        return Ok(ResolvedWorkspace {
            path: input.path,
            connection: Some(connection),
            remote: None,
        });
    }
    if kind(&input.path)? == Kind::Local {
        return Ok(ResolvedWorkspace {
            path: input.path,
            connection: None,
            remote: None,
        });
    }
    let registration = RemoteRegistration::load(&input.path)?;
    let directory = fs::canonicalize(&input.path)
        .context("failed to resolve Remote workspace directory")?;
    let executable = registration
        .ssh_executable()
        .unwrap_or(&input.default_executable)
        .to_owned();
    let connection = ConnectionSpec {
        ssh: registration.ssh().to_owned(),
        executable: Some(executable.clone()),
        directory: Some(directory),
    };
    connection
        .connection(&input.default_executable)
        .map_err(|error| anyhow::anyhow!("invalid Remote workspace connection: {error}"))?;
    let view = RemoteView {
        path: registration.path().to_owned(),
        ssh: registration.ssh().to_owned(),
        destination: mcpm_exec::ssh::Connection::new(
            registration.ssh().to_owned(),
            executable.clone(),
        )
        .destination(),
        executable,
    };
    tracing::info!(workspace = %input.path.display(), "Using saved SSH connection for Remote workspace");
    Ok(ResolvedWorkspace {
        path: PathBuf::from(registration.path()),
        connection: Some(connection),
        remote: Some(view),
    })
}

/// A workspace could not be resolved.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ResolveError(#[from] pub anyhow::Error);
