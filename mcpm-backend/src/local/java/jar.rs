//! JAR document reads, Manifest parsing and Class-Path interpretation.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::local::archive::zip::{Archive, ArchiveError};

pub(crate) const MAX_DOCUMENT: u64 = 4 * 1024 * 1024;

pub(crate) fn open(path: &Path) -> Result<Archive> {
    Archive::open(path).map_err(|source| {
        let context = match &source {
            ArchiveError::Zip(_) => format!("invalid JAR {path:?}"),
            _ => format!("failed to open JAR {path:?}"),
        };
        anyhow::Error::new(source).context(context)
    })
}

pub(crate) fn read(path: &Path, member: &str) -> Result<Option<Vec<u8>>> {
    open(path)?
        .read(member, Some(MAX_DOCUMENT))
        .with_context(|| format!("failed to read {member} in {path:?}"))
}

/// Reads UTF-8 metadata, retaining the existing tolerance for an initial BOM.
pub(crate) fn read_text(archive: &mut Archive, member: &str) -> Result<Option<String>> {
    archive
        .read(member, None)
        .with_context(|| format!("failed to read metadata document {member}"))?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map(|text| text.trim_start_matches('\u{feff}').to_owned())
                .with_context(|| format!("failed to decode metadata document {member}"))
        })
        .transpose()
}

pub(crate) fn manifest(path: &Path) -> Result<HashMap<String, String>> {
    let bytes = read(path, "META-INF/MANIFEST.MF")?
        .with_context(|| format!("JAR {path:?} has no manifest"))?;
    (|| attributes(std::str::from_utf8(&bytes)?))()
        .with_context(|| format!("failed to parse META-INF/MANIFEST.MF in {path:?}"))
}

/// Main section only; continuations are unfolded and repeated names keep the last value.
pub(crate) fn attributes(text: &str) -> Result<HashMap<String, String>> {
    let mut unfolded = Vec::<String>::new();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix(' ') {
            unfolded
                .last_mut()
                .context("orphan manifest continuation")?
                .push_str(rest);
        } else {
            unfolded.push(line.to_owned());
        }
    }
    let mut values = HashMap::new();
    for line in unfolded {
        let (key, value) = line
            .split_once(": ")
            .context("invalid manifest attribute")?;
        values.insert(key.to_ascii_lowercase(), value.to_owned());
    }
    Ok(values)
}

/// Extracts local Class-Path entries without validating the full declaration.
/// Unsupported URI/encoded entries are unavailable evidence rather than command errors.
pub(crate) fn manifest_paths(jar: &Path, manifest: &HashMap<String, String>) -> Vec<PathBuf> {
    let Some(parent) = jar.parent() else {
        return Vec::new();
    };
    manifest
        .get("class-path")
        .into_iter()
        .flat_map(|value| value.split_whitespace())
        .filter_map(|value| {
            if value.contains(':') || value.contains('%') {
                tracing::debug!(
                    ?jar,
                    entry = value,
                    "skipping unsupported Manifest Class-Path entry"
                );
                None
            } else {
                Some(parent.join(value))
            }
        })
        .collect()
}
