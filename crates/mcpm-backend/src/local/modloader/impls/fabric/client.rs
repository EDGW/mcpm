use super::super::utils::coordinate;
use super::FabricError;
use crate::local::mc::client::MinecraftVersionManifest;
use anyhow::Result;

pub(super) fn identify(manifest: &MinecraftVersionManifest) -> Result<Option<String>> {
    // HMCL distinguishes Legacy Fabric by any net.legacyfabric library.
    if manifest
        .libraries()
        .any(|library| library.name.split(':').next() == Some("net.legacyfabric"))
    {
        return Ok(None);
    }
    coordinate::identify(manifest, &["net.fabricmc:fabric-loader"])
        .map_err(|source| FabricError::ClientIdentification { source }.into())
}
