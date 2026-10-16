//! Resolves server mod containers and paths using registered server metadata.

use std::{
    collections::HashMap,
    path::{Component, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mc::server::MinecraftServer,
    mcmod::ModContainer,
    modloader::{LoaderId, ModLoaderMeta},
};
use thiserror::Error;

/// Registered server and loader scope shared by server mod commands.
pub(super) struct ServerMods {
    container: ModContainer,
}

impl ServerMods {
    pub(super) fn load(path: PathBuf) -> Result<Self> {
        let server = MinecraftServer::load(path)?;
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
        Ok(Self { container })
    }

    pub(super) fn loaders(&self) -> HashMap<LoaderId, ModLoaderMeta> {
        let loader = self.container.loader().loader.clone();
        [(loader.id().clone(), loader)].into_iter().collect()
    }

    pub(super) fn entries(&self) -> Result<Vec<kako_mods_lib::local::mcmod::ModEntry>> {
        self.container.mods().with_context(|| {
            format!(
                "failed to read server mod directory {:?}",
                self.container.directory().path()
            )
        })
    }

    fn files(&self, files: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        if files.is_empty() {
            return Ok(self
                .entries()?
                .into_iter()
                .map(|entry| entry.path().to_owned())
                .collect());
        }
        files
            .into_iter()
            .map(|path| {
                if path.as_os_str().is_empty()
                    || path
                        .components()
                        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
                {
                    return Err(ServerModsError::InvalidModPath { path }.into());
                }
                let relative: PathBuf = path
                    .components()
                    .filter(|part| !matches!(part, Component::CurDir))
                    .collect();
                // Accept both a mods-relative name and a server-relative mods/name.
                let relative = relative.strip_prefix("mods").unwrap_or(&relative);
                Ok(self.container.directory().path().join(relative))
            })
            .collect()
    }
}

/// Input selection for server mod information and dependency inspection.
#[derive(Args)]
pub(super) struct FileArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Mod filenames or mods/FILE paths; omit to inspect all mod files.
    #[arg(value_name = "FILE", num_args = 0..)]
    files: Vec<PathBuf>,
}

impl FileArgs {
    pub(super) fn resolve(self) -> Result<(ServerMods, Vec<PathBuf>)> {
        let source = ServerMods::load(self.path)?;
        let files = source.files(self.files)?;
        Ok((source, files))
    }
}

#[derive(Debug, Error)]
enum ServerModsError {
    #[error("server {path:?} has no declared mod loader")]
    NoLoader { path: PathBuf },
    #[error("server {path:?} declares unsupported mod loader {loader:?}")]
    UnsupportedLoader { path: PathBuf, loader: String },
    #[error(
        "mod file {path:?} must be relative to the server's mods directory without parent traversal"
    )]
    InvalidModPath { path: PathBuf },
}
