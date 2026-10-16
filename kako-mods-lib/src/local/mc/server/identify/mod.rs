//! Read-only identification from a statically understood start script.
//! This never runs shell commands, Java, installers, or downloads.

pub(crate) mod archive;
mod java;
mod script;
mod tokens;

use super::{MinecraftServerMetadata, ServerModLoader, ServerStartCommand};
use crate::local::modloader::{LoaderId, ModLoaderMeta};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub use java::ServerLaunch;

/// An identified launch. Source location and Java entry point explain its provenance.
/// Identification does not guarantee that dependencies are installed or launchable.
#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub metadata: MinecraftServerMetadata,
    pub launch: ServerLaunch,
}

#[derive(Debug, Error)]
pub enum ServerIdentificationError {
    #[error("no Java server launch found in {path:?}")]
    NoLaunch { path: PathBuf },
    #[error(
        "cannot accurately identify server at {path:?}:{line}; no recognized loader or vanilla entry"
    )]
    Unrecognized { path: PathBuf, line: usize },
    #[error("multiple loader implementations recognized the same Java launch at {path:?}:{line}")]
    ConflictingLoaders { path: PathBuf, line: usize },
}

/// `script` is relative to `root` unless absolute. All script/file paths are resolved
/// without executing anything. Unknown syntax, missing identity evidence, conflicting versions,
/// or any unrecognized Java launch fail the entire operation without partial results.
/// Loader implementations are supplied by the caller; no built-ins are assumed.
pub fn identify(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Vec<ServerCandidate>> {
    tracing::info!(root = ?root.as_ref(), script = ?script.as_ref(), "identifying Minecraft server");
    let result = identify_inner(root.as_ref(), script.as_ref(), loaders);
    match &result {
        Ok(candidates) => {
            tracing::info!(count = candidates.len(), "server identification completed")
        }
        Err(error) => {
            tracing::debug!(error = %format_args!("{error:#}"), "server identification failed")
        }
    }
    result
}

fn identify_inner(
    root: &Path,
    script: &Path,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Vec<ServerCandidate>> {
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("failed to resolve server directory {root:?}"))?;
    anyhow::ensure!(root.is_dir(), "server root is not a directory: {root:?}");
    let path = root.join(script);
    let launches = script::read(&root, &path)?;
    if launches.is_empty() {
        return Err(ServerIdentificationError::NoLaunch { path }.into());
    }
    let mut ordered: Vec<_> = loaders.values().collect();
    ordered.sort_by(|a, b| a.id().cmp(b.id()));
    let mut candidates: Vec<ServerCandidate> = Vec::new();
    for launch in launches {
        tracing::debug!(script = ?launch.script, line = launch.line, main_class = %launch.main_class, "inspecting Java server entry");
        let mut recognized = Vec::new();
        for loader in &ordered {
            tracing::trace!(loader = %loader.name(), "checking server loader evidence");
            if let Some(identity) = loader
                .instance()
                .try_identify_server(&launch)
                .with_context(|| {
                    format!(
                        "failed to identify {} at {:?}:{}",
                        loader.name(),
                        launch.script,
                        launch.line
                    )
                })?
            {
                tracing::info!(loader = %loader.name(), minecraft_version = %identity.minecraft_version, loader_version = %identity.version, "identified server loader");
                recognized.push(MinecraftServerMetadata {
                    version: 1,
                    minecraft_version: identity.minecraft_version,
                    loader: Some(ServerModLoader {
                        loader_type: loader.id().to_string(),
                        version: identity.version,
                    }),
                    start: None,
                });
            }
        }
        if recognized.len() > 1 {
            return Err(ServerIdentificationError::ConflictingLoaders {
                path: launch.script,
                line: launch.line,
            }
            .into());
        }
        let mut metadata = match recognized.pop() {
            Some(metadata) => metadata,
            None => {
                let Some(version) = java::vanilla(&launch).with_context(|| {
                    format!(
                        "failed to identify vanilla server at {:?}:{}",
                        launch.script, launch.line
                    )
                })?
                else {
                    return Err(ServerIdentificationError::Unrecognized {
                        path: launch.script,
                        line: launch.line,
                    }
                    .into());
                };
                MinecraftServerMetadata {
                    version: 1,
                    minecraft_version: version,
                    loader: None,
                    start: None,
                }
            }
        };
        metadata.start = Some(ServerStartCommand {
            script: launch.script.clone(),
            directory: launch.directory.clone(),
            executable: launch.executable.clone(),
            arguments: launch.arguments.clone(),
            windows: launch.path_separator == ';',
        });
        if !candidates.iter().any(|existing| {
            existing.metadata == metadata
                && existing.launch.directory == launch.directory
                && existing.launch.arguments == launch.arguments
        }) {
            candidates.push(ServerCandidate { metadata, launch });
        }
    }
    Ok(candidates)
}
