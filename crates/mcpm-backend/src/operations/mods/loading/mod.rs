//! Shared Host-side Mod scopes, selection and loading, independent of CLI and presentation.

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use mcpm_lib::local::{
    cache::mods::ModsCacheSelection,
    mcmod::{
        ModFile,
        dependency::graph::{candidate::ExternalCandidate, identifier::DependencyIdentifier},
    },
    modloader::{BUILTIN_LOADERS, LoaderId, ModLoaderMeta},
    runtime::RuntimeEnvironment,
};
use serde::{Deserialize, Serialize};

pub mod inspection;
pub(crate) mod list_selection;
pub mod mod_records;
pub mod scan;
pub mod selection;

/// The file-owning scope; paths are interpreted only by the execution host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScopeInput {
    Directory {
        directory: PathBuf,
        loaders: Vec<String>,
        cache: Option<ModsCacheSelection>,
    },
    Server {
        path: PathBuf,
        no_cache: bool,
    },
}

impl ScopeInput {
    /// Resolve loader, runtime and container rules through their existing owners.
    pub fn resolve(self) -> Result<ResolvedScope> {
        match self {
            Self::Directory {
                directory,
                loaders,
                cache,
            } => {
                let hide_unrecognized = !loaders.is_empty();
                let selected = if hide_unrecognized {
                    loaders
                        .into_iter()
                        .map(|id| {
                            let loader = BUILTIN_LOADERS
                                .get(id.as_str())
                                .with_context(|| format!("unavailable mod loader: {id}"))?;
                            Ok((loader.id().clone(), loader.clone()))
                        })
                        .collect::<Result<_>>()?
                } else {
                    BUILTIN_LOADERS.clone()
                };
                Ok(ResolvedScope {
                    scope: selection::Scope::directory(directory.clone()),
                    directory,
                    loaders: selected,
                    environment: RuntimeEnvironment::new().with_mods_cache(cache),
                    hide_unrecognized,
                    external: Vec::new(),
                })
            }
            Self::Server { path, no_cache } => {
                let server = crate::workspace::load_local(path)?;
                let container = server
                    .mods()
                    .with_context(|| match &server.metadata().loader {
                        None => format!("server {:?} has no declared mod loader", server.path()),
                        Some(loader) => format!(
                            "server {:?} declares unsupported mod loader {:?}",
                            server.path(),
                            loader.loader_type
                        ),
                    })?;
                let loader = container.loader();
                let loader_id = loader.loader.id().clone();
                let loader_modid = match loader_id.as_str() {
                    "fabric" => "fabricloader",
                    id => id,
                };
                let external = [
                    ("minecraft", server.metadata().minecraft_version.as_str()),
                    (loader_modid, loader.version.as_str()),
                ]
                .into_iter()
                .map(|(id, version)| ExternalCandidate {
                    identifier: DependencyIdentifier::from_parts(loader_id.clone(), id.to_owned()),
                    version: Some(version.to_owned()),
                })
                .collect();
                let directory = container.directory().path().to_owned();
                let environment = if no_cache {
                    server.runtime().clone().with_mods_cache(None)
                } else {
                    server.runtime().clone()
                };
                Ok(ResolvedScope {
                    scope: selection::Scope::container(directory.clone()),
                    directory,
                    loaders: [(loader_id, loader.loader.clone())].into_iter().collect(),
                    environment,
                    hide_unrecognized: true,
                    external,
                })
            }
        }
    }
}

/// Resolved resources stay within backend work and never cross the transport.
pub struct ResolvedScope {
    pub scope: selection::Scope,
    pub directory: PathBuf,
    pub loaders: HashMap<LoaderId, ModLoaderMeta>,
    pub environment: RuntimeEnvironment,
    pub hide_unrecognized: bool,
    pub external: Vec<ExternalCandidate>,
}

/// Preserve the library's stable metadata presentation order after acquisition.
pub fn sort_metadata(file: &mut ModFile) {
    fn order(id: &LoaderId) -> usize {
        match id.as_str() {
            "forge" => 0,
            "neoforge" => 1,
            "fabric" => 2,
            _ => 4,
        }
    }
    file.metadata.sort_by(|left, right| {
        order(left.loader.id())
            .cmp(&order(right.loader.id()))
            .then_with(|| left.loader.id().cmp(right.loader.id()))
    });
}

/// Freeze the Host cache identity before subsequent source lookups.
pub fn cache_selection(environment: &RuntimeEnvironment) -> Result<Option<ModsCacheSelection>> {
    Ok(environment
        .mods_cache()?
        .map(|cache| ModsCacheSelection::Directory {
            path: cache.path().to_owned(),
        }))
}
