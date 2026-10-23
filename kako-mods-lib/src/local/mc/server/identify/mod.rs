//! Read-only identification from a statically understood start script.
//! This never runs shell commands, Java, installers, or downloads.

mod java;
mod script;
mod tokens;
mod vanilla;

use super::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader, ServerStartCommand,
};
use crate::local::modloader::{LoaderId, ModLoaderMeta};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

// Preserve the existing public import path.
pub use super::ServerLaunch;

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
/// without executing anything. This extracts identity evidence, not command validity.
/// Unavailable commands are diagnosed independently; recognized candidates survive
/// failures elsewhere. When no candidate is found, an extraction diagnostic is returned.
/// The caller selects loader implementations from the complete BUILTIN_LOADERS registry.
pub fn identify(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Vec<ServerCandidate>> {
    tracing::info!(root = ?root.as_ref(), script = ?script.as_ref(), "identifying Minecraft server");
    let result = identify_inner(root.as_ref(), script.as_ref(), loaders);
    match &result {
        Ok(candidates) => {
            tracing::debug!(count = candidates.len(), "server identification completed")
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
    let evidence = script::read(&root, &path);
    let mut diagnostics = evidence.diagnostics;
    let mut ordered: Vec<_> = loaders.values().collect();
    ordered.sort_by(|a, b| a.id().cmp(b.id()));
    let mut candidates: Vec<ServerCandidate> = Vec::new();
    for launch in evidence.launches {
        tracing::debug!(script = ?launch.script, line = launch.line, main_class = %launch.main_class, "inspecting Java server entry");
        let mut metadata = match identify_launch(&launch, &ordered) {
            Ok(Some(metadata)) => metadata,
            Ok(None) => {
                diagnostics.push(
                    ServerIdentificationError::Unrecognized {
                        path: launch.script,
                        line: launch.line,
                    }
                    .into(),
                );
                continue;
            }
            Err(error) => {
                tracing::debug!(error = %format_args!("{error:#}"), "skipping unavailable launch identity");
                diagnostics.push(error);
                continue;
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
    if candidates.is_empty() {
        return Err(diagnostics
            .into_iter()
            .next()
            .unwrap_or_else(|| ServerIdentificationError::NoLaunch { path }.into()));
    }
    Ok(candidates)
}

/// One command's extraction failure must not discard other candidates.
fn identify_launch(
    launch: &ServerLaunch,
    ordered: &[&ModLoaderMeta],
) -> Result<Option<MinecraftServerMetadata>> {
    let mut recognized = Vec::new();
    for loader in ordered {
        tracing::trace!(loader = %loader.name(), "checking server loader evidence");
        if let Some(identity) =
            loader
                .instance()
                .try_identify_server(launch)
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
                version: SERVER_METADATA_VERSION,
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
            path: launch.script.clone(),
            line: launch.line,
        }
        .into());
    }
    let metadata = match recognized.pop() {
        Some(metadata) => metadata,
        None => {
            let Some(version) = vanilla::vanilla(launch).with_context(|| {
                format!(
                    "failed to identify vanilla server at {:?}:{}",
                    launch.script, launch.line
                )
            })?
            else {
                return Ok(None);
            };
            MinecraftServerMetadata {
                version: SERVER_METADATA_VERSION,
                minecraft_version: version,
                loader: None,
                start: None,
            }
        }
    };
    Ok(Some(metadata))
}
