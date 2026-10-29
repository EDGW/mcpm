//! Complete server recognition: start-script launches plus mcpm's registration format.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use kako_mc_lib::local::{
    mc::server::{
        ServerLaunch,
        identify::{self, ServerIdentificationError},
        script_path,
    },
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader, ServerStartCommand,
    config::{self, ConfigurationDraft},
};

/// A recognized launch with the registration metadata and configuration it implies.
/// Identification does not guarantee that dependencies are installed or launchable.
#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub metadata: MinecraftServerMetadata,
    pub launch: ServerLaunch,
    pub configuration: ConfigurationDraft,
}

/// Identifies every server a start script declares, without writing anything.
///
/// `script` must be relative to and contained within `root`. Unavailable commands
/// are diagnosed independently; recognized candidates survive failures elsewhere.
/// The caller selects loader implementations from the complete `BUILTIN_LOADERS` registry.
///
/// # Errors
///
/// Returns the first diagnostic when no candidate survives, including when the
/// server root or script path cannot be resolved.
pub fn identify(
    root: impl AsRef<Path>,
    script: impl AsRef<Path>,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
) -> Result<Vec<ServerCandidate>> {
    let scan = identify::scan(root.as_ref(), script.as_ref(), loaders)?;
    let root = std::fs::canonicalize(root.as_ref())
        .with_context(|| format!("failed to resolve server directory {:?}", root.as_ref()))?;
    let path = root.join(script.as_ref());
    let mut diagnostics = scan.diagnostics;
    let mut candidates: Vec<ServerCandidate> = Vec::new();
    for launch in scan.launches {
        let script = launch.launch.script.strip_prefix(&root).with_context(|| {
            format!(
                "identified start script {:?} escapes Server directory {root:?}",
                launch.launch.script
            )
        })?;
        script_path::validate(&root, script)?;
        let metadata = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: launch.identity.minecraft_version,
            loader: launch.identity.loader.map(|loader| ServerModLoader {
                loader_type: loader.loader.id().as_str().to_owned(),
                version: loader.version,
            }),
            start: Some(ServerStartCommand {
                script: script.to_owned(),
                executable: launch.launch.executable.clone(),
                arguments: launch.launch.command.arguments(),
                windows: launch.launch.path_separator == ';',
            }),
        };
        if candidates.iter().any(|existing| {
            existing.metadata == metadata
                && existing.launch.directory == launch.launch.directory
                && existing.launch.command == launch.launch.command
                && existing.launch.environment == launch.launch.environment
        }) {
            continue;
        }
        let configuration = match config::draft(&root, &launch.launch) {
            Ok(draft) => draft,
            Err(error) => {
                diagnostics.push(error.context(format!(
                    "cannot convert {:?}:{} to launch configuration",
                    launch.launch.script, launch.launch.line
                )));
                continue;
            }
        };
        candidates.push(ServerCandidate {
            metadata,
            configuration,
            launch: launch.launch,
        });
    }
    if candidates.is_empty() {
        return Err(diagnostics
            .into_iter()
            .next()
            .unwrap_or_else(|| ServerIdentificationError::NoLaunch { path }.into()));
    }
    Ok(candidates)
}
