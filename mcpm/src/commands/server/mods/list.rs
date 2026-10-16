//! Resolves a server's mod container for the shared CLI mod listing pipeline.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mc::server::MinecraftServer;
use thiserror::Error;

use crate::commands::mod_support::list::{self, ListDisplayArgs};

#[derive(Args)]
pub(super) struct ListArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(value_name = "SERVER_DIR_PATH", default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    display: ListDisplayArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let server = MinecraftServer::load(args.path)?;
    let container = server
        .mods()
        .ok_or_else(|| match &server.metadata().loader {
            None => ListError::NoLoader {
                path: server.path().to_owned(),
            },
            Some(loader) => ListError::UnsupportedLoader {
                path: server.path().to_owned(),
                loader: loader.loader_type.clone(),
            },
        })?;
    let loader = container.loader().loader.clone();
    let loaders = [(loader.id().clone(), loader)].into_iter().collect();
    let input = container.mods().with_context(|| {
        format!(
            "failed to read server mod directory {:?}",
            container.directory().path()
        )
    });
    list::run([input], loaders, true, args.display)
}

#[derive(Debug, Error)]
enum ListError {
    #[error("server {path:?} has no declared mod loader")]
    NoLoader { path: PathBuf },
    #[error("server {path:?} declares unsupported mod loader {loader:?}")]
    UnsupportedLoader { path: PathBuf, loader: String },
}
