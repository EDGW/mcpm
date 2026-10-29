//! `server.mods.context`: the compatibility facts a source command needs from a server.
//!
//! A surface source query needs the Minecraft version, loader and cache selection but no
//! file content, so this operation returns exactly those facts and keeps the server's
//! directory private.

use std::{future::Future, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::data::server::MinecraftServer;
use crate::data::workspace::{self, Kind};
use crate::error::OperationError;
use crate::scope::{HostCache, ScopeKind};

use super::types::ModsContext;

/// Resolves the source-compatibility context of a registered server.
#[derive(Debug)]
pub struct ModsContextOperation;

crate::host_operation!(ModsContextOperation, "server.mods.context");

impl Operation for ModsContextOperation {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = ContextInput;
    type Event = ();
    type Output = ModsContext;
    type Error = OperationError<ContextKind>;

    fn run(
        input: ContextInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || context(input))
                .await
                .map_err(|error: ContextFailure| error.into_operation())
        }
    }
}

/// One context request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
}

fn context(input: ContextInput) -> Result<ModsContext, ContextFailure> {
    match workspace::kind(&input.path) {
        Ok(Kind::Local) => {}
        Ok(Kind::Remote) => {
            return Err(ContextFailure::Remote { path: input.path });
        }
        Err(error) => return Err(ContextFailure::Registration(error)),
    }
    let server = MinecraftServer::load(input.path.clone()).map_err(ContextFailure::Server)?;
    let container = server.mods().ok_or_else(|| match &server.metadata().loader {
        None => ContextFailure::NoLoader {
            path: input.path.clone(),
        },
        Some(loader) => ContextFailure::UnsupportedServerLoader {
            path: input.path.clone(),
            loader: loader.loader_type.clone(),
        },
    })?;
    let loader = container.loader().loader.id().as_str().to_owned();
    let cache = HostCache::capture(server.runtime()).map_err(ContextFailure::Cache)?;
    Ok(ModsContext {
        directory: container.directory().path().to_owned(),
        loader,
        minecraft_version: server.metadata().minecraft_version.clone(),
        cache,
    })
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, thiserror::Error)]
enum ContextFailure {
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    Remote { path: PathBuf },
    #[error(transparent)]
    Registration(anyhow::Error),
    #[error(transparent)]
    Server(crate::data::server::MinecraftServerError),
    #[error("server {path:?} has no declared mod loader")]
    NoLoader { path: PathBuf },
    #[error("server {path:?} declares unsupported mod loader {loader:?}")]
    UnsupportedServerLoader { path: PathBuf, loader: String },
    #[error("failed to open the Mod cache")]
    Cache(#[source] anyhow::Error),
}

impl ContextFailure {
    fn into_operation(self) -> OperationError<ContextKind> {
        let kind = match &self {
            Self::Remote { .. } => ContextKind::Scope(ScopeKind::RemoteWorkspace),
            Self::Registration(_) | Self::Server(_) => {
                ContextKind::Scope(ScopeKind::Registration)
            }
            Self::NoLoader { .. } => ContextKind::Scope(ScopeKind::MissingLoader),
            Self::UnsupportedServerLoader { .. } => {
                ContextKind::Scope(ScopeKind::UnsupportedServerLoader)
            }
            Self::Cache(_) => ContextKind::Scope(ScopeKind::Cache),
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a context failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Scope(ScopeKind),
}
