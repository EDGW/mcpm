//! Host-side resolution of [`ModsScope`] into directories, loaders and an environment.
//!
//! Server scope is resolved where the server lives, so mods commands never need a second
//! round trip to learn the directory or the fixed loader. The caller keeps the resulting
//! environment local; only operation inputs and outputs cross a process boundary.

use std::{
    collections::HashMap,
    path::PathBuf,
};

use anyhow::Result;
use kako_mc_lib::local::modloader::{BUILTIN_LOADERS, LoaderId, ModLoaderMeta};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::data::cache::mods::ModsCacheSelection;
use crate::data::runtime::RuntimeEnvironment;
use crate::data::server::MinecraftServer;
use crate::data::server::MinecraftServerError;
use crate::data::workspace::{self, Kind, WorkspaceError};
use crate::mods::types::{ModsCachePolicy, ModsScope};

/// Everything a mod-reading operation needs on the owning host.
#[derive(Debug)]
pub struct ResolvedScope {
    pub directory: PathBuf,
    pub loaders: HashMap<LoaderId, ModLoaderMeta>,
    /// True when the caller selected loaders explicitly.
    pub loaders_filtered: bool,
    pub environment: RuntimeEnvironment,
}

/// Resolves a scope and applies the cache policy.
pub fn resolve(scope: &ModsScope, policy: &ModsCachePolicy) -> Result<ResolvedScope, ScopeError> {
    match scope {
        ModsScope::Paths {
            directory, loaders, ..
        } => {
            let (loaders, filtered) = select_loaders(loaders)?;
            let directory = directory.clone();
            let environment = apply_policy(RuntimeEnvironment::new(), policy)?;
            Ok(ResolvedScope {
                directory,
                loaders,
                loaders_filtered: filtered,
                environment,
            })
        }
        ModsScope::Server { path } => {
            require_local(path)?;
            let server = MinecraftServer::load(path.clone())?;
            let container = server.mods().ok_or_else(|| match &server.metadata().loader {
                None => ScopeError::NoLoader { path: path.clone() },
                Some(loader) => ScopeError::UnsupportedServerLoader {
                    path: path.clone(),
                    loader: loader.loader_type.clone(),
                },
            })?;
            let loader = container.loader().loader.clone();
            let loaders = [(loader.id().clone(), loader)].into_iter().collect();
            let environment = apply_policy(server.runtime().clone(), policy)?;
            Ok(ResolvedScope {
                directory: container.directory().path().to_owned(),
                loaders,
                loaders_filtered: true,
                environment,
            })
        }
    }
}

fn select_loaders(
    requested: &[String],
) -> Result<(HashMap<LoaderId, ModLoaderMeta>, bool), ScopeError> {
    if requested.is_empty() {
        return Ok((BUILTIN_LOADERS.clone(), false));
    }
    let mut selected = HashMap::new();
    for id in requested {
        let loader = BUILTIN_LOADERS
            .get(id.as_str())
            .ok_or_else(|| ScopeError::UnsupportedLoader { id: id.clone() })?;
        selected.insert(loader.id().clone(), loader.clone());
    }
    Ok((selected, true))
}

fn apply_policy(
    environment: RuntimeEnvironment,
    policy: &ModsCachePolicy,
) -> Result<RuntimeEnvironment, ScopeError> {
    let environment = match policy {
        ModsCachePolicy::ScopeDefault => environment,
        ModsCachePolicy::Disabled => environment.with_mods_cache(None),
        ModsCachePolicy::Select { selection } => {
            environment.with_mods_cache(Some(selection.clone()))
        }
    };
    environment
        .mods_cache()
        .map_err(|source| ScopeError::Cache { source })?;
    Ok(environment)
}

fn require_local(path: &std::path::Path) -> Result<(), ScopeError> {
    match workspace::kind(path) {
        Ok(Kind::Local) => Ok(()),
        Ok(Kind::Remote) => Err(ScopeError::RemoteWorkspace(
            crate::data::workspace::WorkspaceError::RemoteTargetNotLocal {
                path: path.to_owned(),
            },
        )),
        Err(source) => Err(ScopeError::Marker { source }),
    }
}

/// The cache selection resolved on the file host, without local resource handles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCache {
    pub cache: Option<ModsCacheSelection>,
}

impl HostCache {
    pub fn capture(environment: &RuntimeEnvironment) -> Result<Self> {
        Ok(Self {
            cache: environment
                .mods_cache()?
                .map(|cache| ModsCacheSelection::Directory {
                    path: cache.path().to_owned(),
                }),
        })
    }

    pub fn into_runtime(self) -> RuntimeEnvironment {
        RuntimeEnvironment::new().with_mods_cache(self.cache)
    }
}

/// Why a scope could not be resolved on the host.
#[derive(Debug, Error)]
pub enum ScopeError {
    #[error("unavailable mod loader: {id}")]
    UnsupportedLoader { id: String },
    #[error("server {path:?} has no declared mod loader")]
    NoLoader { path: PathBuf },
    #[error("server {path:?} declares unsupported mod loader {loader:?}")]
    UnsupportedServerLoader { path: PathBuf, loader: String },
    #[error(transparent)]
    RemoteWorkspace(#[from] WorkspaceError),
    #[error(transparent)]
    Server(#[from] MinecraftServerError),
    #[error("failed to inspect the workspace manifest")]
    Marker {
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to open the Mod cache")]
    Cache {
        #[source]
        source: anyhow::Error,
    },
}

impl ScopeError {
    /// Stable classification for callers that need to choose a message or exit policy.
    pub fn kind(&self) -> ScopeKind {
        match self {
            Self::UnsupportedLoader { .. } => ScopeKind::UnsupportedLoader,
            Self::NoLoader { .. } => ScopeKind::MissingLoader,
            Self::UnsupportedServerLoader { .. } => ScopeKind::UnsupportedServerLoader,
            Self::RemoteWorkspace(_) => ScopeKind::RemoteWorkspace,
            Self::Server(_) => ScopeKind::Registration,
            Self::Marker { .. } => ScopeKind::Registration,
            Self::Cache { .. } => ScopeKind::Cache,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    UnsupportedLoader,
    MissingLoader,
    UnsupportedServerLoader,
    RemoteWorkspace,
    Registration,
    Cache,
}
