//! Read-only identification from a statically understood start script.
//! This never runs shell commands, Java, installers, or downloads.

mod java;
mod script;
mod tokens;
mod vanilla;

pub(crate) use vanilla::is_vanilla_main_class;

use super::{MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader, ServerModLoader};
use crate::local::modloader::{LoaderId, ModLoaderMeta};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

// Preserve the existing public import path.
pub use super::ServerLaunch;

/// A launch and one of its recognized identities. Source location and Java entry explain its provenance.
/// Identification does not guarantee that dependencies are installed or launchable.
#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub metadata: MinecraftServerMetadata,
    pub launch: ServerLaunch,
    pub configuration: super::config::ConfigurationDraft,
}

/// Controls how the script scanner handles statements it cannot interpret.
pub enum ScriptPolicy<'a> {
    Strict,
    KeepKnownState(&'a mut Vec<SkippedStatement>),
}

/// A statement ignored while keeping the previously known script state.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkippedStatement {
    pub script: PathBuf,
    pub line: usize,
    pub reason: String,
}

/// The earlier statement that made state needed for launch discovery unknown.
#[derive(Clone, Debug, Error)]
#[error("script state became unknown at {script:?}:{line}")]
pub struct UncertainScriptState {
    script: PathBuf,
    line: usize,
}

/// `script` must be relative to and contained within `root`. All script/file paths are resolved
/// without executing anything. This extracts identity evidence, not command validity.
/// A launch yields one candidate per matching loader. Unavailable commands are diagnosed
/// independently; if no candidate is found, an extraction diagnostic is returned.
/// The caller selects loader implementations from the complete BUILTIN_LOADERS registry.
pub fn identify(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Vec<ServerCandidate>> {
    identify_with_policy(root, script, loaders, ScriptPolicy::Strict)
}

/// Identifies server launch candidates with the given script-state policy.
pub fn identify_with_policy(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    mut policy: ScriptPolicy<'_>,
) -> Result<Vec<ServerCandidate>> {
    tracing::info!(root = ?root.as_ref(), script = ?script.as_ref(), "identifying Minecraft server");
    let result = identify_inner(root.as_ref(), script.as_ref(), loaders, &mut policy);
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
    policy: &mut ScriptPolicy<'_>,
) -> Result<Vec<ServerCandidate>> {
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("failed to resolve server directory {root:?}"))?;
    anyhow::ensure!(root.is_dir(), "server root is not a directory: {root:?}");
    super::script_path::validate(&root, script)?;
    let path = root.join(script);
    let evidence = script::read(&root, &path, policy);
    let mut diagnostics = evidence.diagnostics;
    let mut ordered: Vec<_> = loaders.values().collect();
    ordered.sort_by(|a, b| a.id().cmp(b.id()));
    let mut candidates: Vec<ServerCandidate> = Vec::new();
    let mut identities = Vec::new();
    for launch in evidence.launches {
        tracing::debug!(script = ?launch.script, line = launch.line, main_class = %launch.main_class(), "inspecting Java server entry");
        identities.clear();
        if let Err(error) = identify_launch(&launch, &ordered, &mut identities) {
            tracing::debug!(error = %format_args!("{error:#}"), "skipping unavailable launch identity");
            diagnostics.push(error);
            continue;
        }
        let script = launch.script.strip_prefix(&root).with_context(|| {
            format!(
                "identified start script {:?} escapes Server directory {root:?}",
                launch.script
            )
        })?;
        super::script_path::validate(&root, script)?;
        identities.retain(|metadata| {
            !candidates.iter().any(|existing| {
                existing.metadata == *metadata
                    && existing.launch.script == launch.script
                    && existing.launch.directory == launch.directory
                    && existing.launch.command == launch.command
                    && existing.launch.environment == launch.environment
            })
        });
        let Some(last) = identities.pop() else {
            continue;
        };
        let configuration = match super::config::draft(&root, &launch) {
            Ok(draft) => draft,
            Err(error) => {
                diagnostics.push(error.context(format!(
                    "cannot convert {:?}:{} to launch configuration",
                    launch.script, launch.line
                )));
                continue;
            }
        };
        candidates.extend(identities.drain(..).map(|metadata| ServerCandidate {
            metadata,
            launch: launch.clone(),
            configuration: configuration.clone(),
        }));
        candidates.push(ServerCandidate {
            metadata: last,
            launch,
            configuration,
        });
    }
    if candidates.is_empty() {
        if let Some(error) = evidence.blocked_launch {
            return Err(error);
        }
        return Err(diagnostics
            .into_iter()
            .next()
            .unwrap_or_else(|| ServerIdentificationError::NoLaunch { path }.into()));
    }
    Ok(candidates)
}

/// Collects this command's matching loader identities before considering vanilla.
fn identify_launch(
    launch: &ServerLaunch,
    ordered: &[&ModLoaderMeta],
    recognized: &mut Vec<MinecraftServerMetadata>,
) -> Result<()> {
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
                loader: ServerLoader::Modded(ServerModLoader {
                    loader_type: loader.id().to_string(),
                    version: identity.version,
                }),
            });
        }
    }
    if !recognized.is_empty() {
        return Ok(());
    }
    let version = vanilla::vanilla(launch)
        .with_context(|| {
            format!(
                "failed to identify vanilla server at {:?}:{}",
                launch.script, launch.line
            )
        })?
        .ok_or_else(|| ServerIdentificationError::Unrecognized {
            path: launch.script.clone(),
            line: launch.line,
        })?;
    recognized.push(MinecraftServerMetadata {
        version: SERVER_METADATA_VERSION,
        minecraft_version: version,
        loader: ServerLoader::Vanilla,
    });
    Ok(())
}

#[derive(Debug, Error)]
pub enum ServerIdentificationError {
    #[error("no Java server launch found in {path:?}")]
    NoLaunch { path: PathBuf },
    #[error(
        "cannot accurately identify server at {path:?}:{line}; no recognized loader or vanilla entry"
    )]
    Unrecognized { path: PathBuf, line: usize },
}
