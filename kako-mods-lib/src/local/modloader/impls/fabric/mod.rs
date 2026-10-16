//! Fabric loader integration for mods, client profiles and server launches.

use super::utils::archive;
use crate::local::{
    mcmod::{
        ModEntry, ModInfo, ModMeta,
        dependency::{Dependency, DependencyMatch},
    },
    modloader::{ModLoader, ModLoaderMeta},
};
use anyhow::Result;
use std::sync::{Arc, LazyLock};

mod client;
mod constraints;
mod error;
mod matching;
mod metadata;
mod server;
pub use error::FabricError;

#[derive(Debug, Default)]
pub struct FabricLoader;

static METADATA: LazyLock<ModLoaderMeta> =
    LazyLock::new(|| ModLoaderMeta::new("fabric".into(), "Fabric".into(), Arc::new(FabricLoader)));

impl ModLoader for FabricLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn matches_dependency(&self, candidate: &ModInfo, dependency: &Dependency) -> DependencyMatch {
        matching::matches(candidate, dependency)
    }

    fn try_identify_server(
        &self,
        launch: &crate::local::mc::server::ServerLaunch,
    ) -> Result<Option<crate::local::modloader::ServerLoaderIdentity>> {
        server::identify(launch)
            .map_err(|source| FabricError::ServerIdentification { source }.into())
    }

    fn try_identify_client(
        &self,
        manifest: &crate::local::mc::client::MinecraftVersionManifest,
    ) -> Result<Option<String>> {
        client::identify(manifest)
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| FabricError::OpenArchive {
            path: entry.path().to_owned(),
            source,
        })?;
        let root = metadata::read_archive(&mut archive, entry.path())?;
        let metadata = archive::collect(
            &mut archive,
            root,
            entry.path(),
            self.metadata(),
            metadata::read_archive,
        )
        .map_err(|source| FabricError::NestedIdentification {
            path: entry.path().to_owned(),
            source,
        })?;
        if !metadata.has_mods() {
            return Ok(None);
        }
        Ok(Some(metadata))
    }
}
