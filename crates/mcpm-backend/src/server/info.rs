//! `server.info`: reads one Local Server's registration for display.
//!
//! The path is resolved on the host that owns the server, so a remote caller receives the
//! same snapshot a local caller gets. Configuration presence is requested explicitly and
//! therefore never inferred for a remote target.

use std::{future::Future, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::data::server::MinecraftServer;
use crate::data::server::MinecraftServerError;
use crate::data::server::config;
use crate::data::workspace::{self, Kind};
use crate::error::OperationError;

use super::types::ServerInfo;

/// Reads a server registration snapshot.
#[derive(Debug)]
pub struct Info;

crate::host_operation!(Info, "server.info");

impl Operation for Info {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = InfoInput;
    type Event = ();
    type Output = ServerInfo;
    type Error = OperationError<InfoKind>;

    fn run(
        input: InfoInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || info(input))
                .await
                .map_err(|error: InfoFailure| error.into_operation())
        }
    }
}

/// One snapshot request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InfoInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    /// Read launch.toml/conf.toml presence; only meaningful for a local target.
    #[serde(default)]
    pub include_configuration: bool,
}

fn info(input: InfoInput) -> Result<ServerInfo, InfoFailure> {
    match workspace::kind(&input.path) {
        Ok(Kind::Local) => {}
        Ok(Kind::Remote) => {
            return Err(InfoFailure::Remote { path: input.path });
        }
        Err(error) => return Err(InfoFailure::Registration(error)),
    }
    tracing::info!(path = %input.path.display(), "Reading server metadata");
    let server = MinecraftServer::inspect(input.path.clone()).map_err(InfoFailure::Server)?;
    let mut info = ServerInfo::from_server(&server);
    if input.include_configuration {
        info.configuration = Some(
            config::configuration_files(server.path()).map_err(InfoFailure::Configuration)?,
        );
    }
    tracing::debug!(name = %info.name, "Loaded server metadata");
    Ok(info)
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, thiserror::Error)]
enum InfoFailure {
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    Remote { path: PathBuf },
    #[error(transparent)]
    Registration(anyhow::Error),
    #[error(transparent)]
    Server(MinecraftServerError),
    #[error("failed to inspect server configuration files")]
    Configuration(#[source] anyhow::Error),
}

impl InfoFailure {
    fn into_operation(self) -> OperationError<InfoKind> {
        let kind = match &self {
            Self::Remote { .. } => InfoKind::RemoteWorkspace,
            Self::Registration(_) | Self::Server(_) => InfoKind::Registration,
            Self::Configuration(_) => InfoKind::Configuration,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of an info failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfoKind {
    RemoteWorkspace,
    Registration,
    Configuration,
}
