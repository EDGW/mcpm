//! Shared coordinate mechanics for HMCL-compatible client detection.
//!
//! Rules follow `LibraryAnalyzer.LibraryType` in HMCL:
//! https://github.com/HMCL-dev/HMCL/blob/main/HMCLCore/src/main/java/org/jackhuang/hmcl/download/LibraryAnalyzer.java

use thiserror::Error;

use crate::local::mc::client::MinecraftVersionManifest;

pub(super) fn has_library(manifest: &MinecraftVersionManifest, coordinates: &[&str]) -> bool {
    manifest.libraries().any(|library| {
        coordinates
            .iter()
            .any(|artifact| matches_coordinate(&library.name, artifact))
    })
}

fn matches_coordinate(name: &str, artifact: &str) -> bool {
    let coordinate = name.split('@').next().unwrap_or(name);
    coordinate == artifact
        || coordinate
            .strip_prefix(artifact)
            .is_some_and(|rest| rest.starts_with(':'))
}

/// HMCL inserts matches into a map while traversing libraries: the last wins.
pub(super) fn identify(
    manifest: &MinecraftVersionManifest,
    coordinates: &[&str],
) -> Result<Option<String>, ClientLibraryError> {
    let mut detected = None;
    for library in manifest.libraries() {
        for artifact in coordinates {
            if !matches_coordinate(&library.name, artifact) {
                continue;
            }
            let coordinate = library.name.split('@').next().unwrap_or(&library.name);
            let version = coordinate
                .strip_prefix(artifact)
                .and_then(|rest| rest.strip_prefix(':'))
                .and_then(|rest| rest.split(':').next())
                .filter(|version| !version.is_empty())
                .ok_or_else(|| ClientLibraryError::InvalidCoordinate {
                    coordinate: library.name.clone(),
                })?;
            detected = Some(version.to_owned());
        }
    }
    Ok(detected)
}

/// Same shape as HMCL's `^([0-9.]+)-(?<forge>[0-9.]+)(-([0-9.]+))?$`.
/// Nonmatching versions retain their entire original string.
pub(super) fn forge_version(version: &str) -> &str {
    let mut parts = version.split('-');
    let minecraft = parts.next().unwrap_or_default();
    let Some(loader) = parts.next() else {
        return version;
    };
    let suffix = parts.next();
    let numeric =
        |part: &str| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit() || ch == b'.');
    if numeric(minecraft) && numeric(loader) && suffix.is_none_or(numeric) && parts.next().is_none()
    {
        loader
    } else {
        version
    }
}

pub(super) const NEOFORGE_LIBRARIES: &[&str] = &[
    "net.neoforged.fancymodloader:core",
    "net.neoforged.fancymodloader:loader",
];

/// HMCL scans string argument pairs in the root first, then in each patch.
/// Conditional objects and legacy `minecraftArguments` are not scanned.
pub(super) fn neoforge_argument_version(manifest: &MinecraftVersionManifest) -> Option<&str> {
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

/// Coordinate errors shared by the built-in client detectors.
#[derive(Debug, Error)]
pub enum ClientLibraryError {
    #[error("invalid loader library coordinate {coordinate:?}")]
    InvalidCoordinate { coordinate: String },
}
