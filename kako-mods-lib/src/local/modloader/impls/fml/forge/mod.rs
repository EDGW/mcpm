//! Forge loader integration for mods, client profiles and server launches.

use super::super::utils::archive;
use crate::local::{
    mcmod::{ModEntry, ModMeta},
    modloader::{ModLoader, ModLoaderMeta},
};
use anyhow::Result;
use std::sync::{Arc, LazyLock};

mod client;
mod error;
mod metadata;
mod server;
pub use error::ForgeError;

#[derive(Debug, Default)]
pub struct ForgeLoader;

static METADATA: LazyLock<ModLoaderMeta> =
    LazyLock::new(|| ModLoaderMeta::new("forge".into(), "Forge".into(), Arc::new(ForgeLoader)));

impl ModLoader for ForgeLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn try_identify_server(
        &self,
        launch: &crate::local::mc::server::identify::ServerLaunch,
    ) -> Result<Option<crate::local::modloader::ServerLoaderIdentity>> {
        server::identify(launch)
            .map_err(|source| ForgeError::ServerIdentification { source }.into())
    }

    fn try_identify_client(
        &self,
        manifest: &crate::local::mc::client::MinecraftVersionManifest,
    ) -> Result<Option<String>> {
        client::identify(manifest)
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| ForgeError::OpenArchive {
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
        .map_err(|source| ForgeError::NestedIdentification {
            path: entry.path().to_owned(),
            source,
        })?;
        if !metadata.has_mods() {
            return Ok(None);
        }
        Ok(Some(metadata))
    }
}
