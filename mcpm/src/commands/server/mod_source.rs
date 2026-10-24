//! Resolves the registered server mod container and versions for server commands.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use mcpm_backend::local::{
    mcmod::ModContainer,
    modloader::{LoaderId, ModLoaderMeta},
};
use thiserror::Error;

use crate::commands::supports::mods::selection::Scope;

/// Registered server and loader scope shared by server mod commands.
#[derive(Debug)]
pub(super) struct ServerMods {
    container: ModContainer,
    minecraft_version: String,
}

impl ServerMods {
    pub(super) fn load(path: PathBuf) -> Result<Self> {
        let server = super::workspace::load_local(path)?;
        let container = server
            .mods()
            .ok_or_else(|| match &server.metadata().loader {
                None => ServerModsError::NoLoader {
                    path: server.path().to_owned(),
                },
                Some(loader) => ServerModsError::UnsupportedLoader {
                    path: server.path().to_owned(),
                    loader: loader.loader_type.clone(),
                },
            })?
            .clone();
        Ok(Self {
            container,
            minecraft_version: server.metadata().minecraft_version.clone(),
        })
    }

    pub(super) fn container(&self) -> &ModContainer {
        &self.container
    }

    pub(super) fn minecraft_version(&self) -> &str {
        &self.minecraft_version
    }

    pub(super) fn loaders(&self) -> HashMap<LoaderId, ModLoaderMeta> {
        let loader = self.container.loader().loader.clone();
        [(loader.id().clone(), loader)].into_iter().collect()
    }

    pub(super) fn scope(&self) -> Scope {
        Scope::container(self.container.directory().path().to_owned())
    }
}

#[derive(Debug, Error)]
enum ServerModsError {
    #[error("server {path:?} has no declared mod loader")]
    NoLoader { path: PathBuf },
    #[error("server {path:?} declares unsupported mod loader {loader:?}")]
    UnsupportedLoader { path: PathBuf, loader: String },
}
