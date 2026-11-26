//! Loader-independent library-coordinate matching and Maven path parsing.
//!
//! Callers supply the artifacts to match; no loader identities or version
//! normalization rules belong here. Client traversal follows HMCL's last-match
//! behavior: https://github.com/HMCL-dev/HMCL/blob/main/HMCLCore/src/main/java/org/jackhuang/hmcl/download/LibraryAnalyzer.java

use anyhow::{Result as AnyResult, ensure};
use std::path::{Component, Path};
use thiserror::Error;

use crate::local::mc::client::MinecraftVersionManifest;

pub(in crate::local::modloader::impls) fn has_library(
    manifest: &MinecraftVersionManifest,
    coordinates: &[&str],
) -> bool {
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
pub(in crate::local::modloader::impls) fn identify(
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

/// Coordinate errors shared by the built-in client detectors.
#[derive(Debug, Error)]
pub enum ClientLibraryError {
    #[error("invalid loader library coordinate {coordinate:?}")]
    InvalidCoordinate { coordinate: String },
}

/// Extracts a version only from a complete referenced Maven artifact path.
pub(in crate::local::modloader::impls) fn coordinate<'a>(
    path: &'a Path,
    group: &str,
    artifact: &str,
) -> Option<&'a str> {
    let (version, file) = artifact_path(path, group, artifact)?;
    let suffix = file
        .strip_suffix(".jar")?
        .strip_prefix(artifact)?
        .strip_prefix('-')?;
    let classifier = suffix.strip_prefix(version)?;
    (classifier.is_empty()
        || classifier
            .strip_prefix('-')
            .is_some_and(|value| !value.is_empty()))
    .then_some(version)
}

/// Matches only expanded Maven argument files, never JAR artifacts.
pub(in crate::local::modloader::impls) fn argument_coordinate<'a>(
    path: &'a Path,
    group: &str,
    artifact: &str,
) -> Option<&'a str> {
    let (version, file) = artifact_path(path, group, artifact)?;
    matches!(file, "unix_args.txt" | "win_args.txt").then_some(version)
}

fn artifact_path<'a>(path: &'a Path, group: &str, artifact: &str) -> Option<(&'a str, &'a str)> {
    // Components normalize trailing separators and `/.`, which denote directories.
    let raw = path.as_os_str().as_encoded_bytes();
    let last = *raw.strip_suffix(b".").unwrap_or(raw).last()?;
    if std::path::is_separator(char::from(last)) {
        return None;
    }
    let mut components = path.components();
    let Component::Normal(file) = components.next_back()? else {
        return None;
    };
    let Component::Normal(version) = components.next_back()? else {
        return None;
    };
    if components.next_back()? != Component::Normal(artifact.as_ref())
        || !group
            .rsplit('.')
            .all(|part| components.next_back() == Some(Component::Normal(part.as_ref())))
    {
        return None;
    }
    Some((version.to_str()?, file.to_str()?))
}

pub(in crate::local::modloader::impls) fn agree(
    current: &mut Option<String>,
    value: &str,
    description: &str,
) -> AnyResult<()> {
    ensure!(!value.trim().is_empty(), "empty {description}");
    if let Some(existing) = current {
        ensure!(
            existing == value,
            "conflicting {description}: {existing:?} versus {value:?}"
        );
    } else {
        *current = Some(value.to_owned());
    }
    Ok(())
}
