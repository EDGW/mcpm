use super::super::super::utils::coordinate;
use super::super::client;
use super::NeoForgeError;
use crate::local::mc::client::MinecraftVersionManifest;
use anyhow::Result;

pub(super) fn identify(manifest: &MinecraftVersionManifest) -> Result<Option<String>> {
    let version = coordinate::identify(manifest, client::NEOFORGE_LIBRARIES)
        .map_err(|source| NeoForgeError::ClientIdentification { source })?;
    Ok(version.map(|version| {
        argument_version(manifest)
            .unwrap_or_else(|| client::forge_version(&version))
            .to_owned()
    }))
}

/// HMCL scans string argument pairs in the root first, then in each patch.
/// Conditional objects and legacy `minecraftArguments` are not scanned.
fn argument_version(manifest: &MinecraftVersionManifest) -> Option<&str> {
    fn scan(manifest: &MinecraftVersionManifest) -> Option<&str> {
        for pair in manifest.game_arguments().windows(2) {
            if matches!(
                pair[0].as_str(),
                Some("--fml.neoForgeVersion" | "--fml.forgeVersion")
            ) {
                return pair[1].as_str();
            }
        }
        None
    }
    scan(manifest).or_else(|| manifest.patches().iter().find_map(scan))
}
