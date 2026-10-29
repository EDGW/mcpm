//! Read-only identification of Java launches from a statically understood start script.
//! This never runs shell commands, Java, installers, or downloads.

mod java;
mod script;
mod tokens;
mod vanilla;

use crate::local::modloader::{LoaderId, LoaderInfo, ModLoaderMeta};
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

// Preserve the existing public import path.
pub use super::ServerLaunch;

/// A recognized Java launch and the server identity established from loader evidence.
#[derive(Debug, Clone)]
pub struct IdentifiedLaunch {
    pub launch: ServerLaunch,
    pub identity: ServerIdentity,
}

/// Registration identity evidence, independent of any stored metadata format.
///
/// Source location and Java entry point of the launch explain its provenance.
/// Identification does not guarantee that dependencies are installed or launchable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerIdentity {
    /// Minecraft version, rather than the loader version.
    pub minecraft_version: String,
    /// The loader that recognized this launch; `None` for a recognized vanilla server.
    pub loader: Option<LoaderInfo>,
}

/// Every identified launch of one start script plus the failures of unnamed launches.
#[derive(Debug)]
pub struct Scan {
    /// Recognized launches in script order, without duplicate commands.
    pub launches: Vec<IdentifiedLaunch>,
    /// Diagnostics for launches that produced no identity, in script order.
    pub diagnostics: Vec<anyhow::Error>,
}

/// `script` must be relative to and contained within `root`. All script/file paths are resolved
/// without executing anything. This extracts identity evidence, not command validity.
/// Unavailable commands are diagnosed independently; recognized launches survive
/// failures elsewhere; the caller decides how to report an empty result.
/// The caller selects loader implementations from the complete BUILTIN_LOADERS registry.
///
/// # Errors
///
/// Returns an error only when the server root or script path cannot be resolved or
/// inspected; per-launch identification failures are collected in [`Scan::diagnostics`].
pub fn scan(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Scan> {
    tracing::info!(root = ?root.as_ref(), script = ?script.as_ref(), "identifying Minecraft server");
    let result = scan_inner(root.as_ref(), script.as_ref(), loaders);
    match &result {
        Ok(scan) => {
            tracing::debug!(count = scan.launches.len(), "server identification completed")
        }
        Err(error) => {
            tracing::debug!(error = %format_args!("{error:#}"), "server identification failed")
        }
    }
    result
}

fn scan_inner(
    root: &Path,
    script: &Path,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Scan> {
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("failed to resolve server directory {root:?}"))?;
    anyhow::ensure!(root.is_dir(), "server root is not a directory: {root:?}");
    super::script_path::validate(&root, script)?;
    let path = root.join(script);
    let evidence = script::read(&root, &path);
    let mut diagnostics = evidence.diagnostics;
    let mut ordered: Vec<_> = loaders.values().collect();
    ordered.sort_by(|a, b| a.id().cmp(b.id()));
    let mut launches: Vec<IdentifiedLaunch> = Vec::new();
    for launch in evidence.launches {
        tracing::debug!(script = ?launch.script, line = launch.line, main_class = %launch.main_class(), "inspecting Java server entry");
        let identity = match identify_launch(&launch, &ordered) {
            Ok(Some(identity)) => identity,
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
        if !launches.iter().any(|existing| {
            existing.identity == identity
                && existing.launch.directory == launch.directory
                && existing.launch.command == launch.command
                && existing.launch.environment == launch.environment
        }) {
            launches.push(IdentifiedLaunch { launch, identity });
        }
    }
    Ok(Scan {
        launches,
        diagnostics,
    })
}

/// One command's extraction failure must not discard other launches.
fn identify_launch(
    launch: &ServerLaunch,
    ordered: &[&ModLoaderMeta],
) -> Result<Option<ServerIdentity>> {
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
            recognized.push(ServerIdentity {
                minecraft_version: identity.minecraft_version,
                loader: Some(LoaderInfo {
                    loader: (*loader).clone(),
                    version: identity.version,
                }),
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
    let identity = match recognized.pop() {
        Some(identity) => identity,
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
            ServerIdentity {
                minecraft_version: version,
                loader: None,
            }
        }
    };
    Ok(Some(identity))
}

/// Failures to establish any identity for a launch found in a start script.
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
