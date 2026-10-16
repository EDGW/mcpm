use super::super::super::utils::coordinate;
use super::super::client;
use super::ForgeError;
use crate::local::mc::client::MinecraftVersionManifest;
use anyhow::Result;

pub(super) fn identify(manifest: &MinecraftVersionManifest) -> Result<Option<String>> {
    if coordinate::has_library(manifest, client::NEOFORGE_LIBRARIES) {
        return Ok(None);
    }
    coordinate::identify(
        manifest,
        &["net.minecraftforge:forge", "net.minecraftforge:fmlloader"],
    )
    .map(|version| version.map(|version| client::forge_version(&version).to_owned()))
    .map_err(|source| ForgeError::ClientIdentification { source }.into())
}
