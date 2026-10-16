//! JAR document reads, Manifest parsing and Class-Path interpretation.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use crate::local::archive::zip::{Archive, ArchiveError};

pub(crate) const MAX_DOCUMENT: u64 = 4 * 1024 * 1024;

/// Server evidence rejects duplicates; FML version substitution keeps the last header.
#[derive(Clone, Copy)]
pub(crate) enum DuplicateHeaders {
    Reject,
    LastWins,
}

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

pub(crate) fn has(path: &Path, member: &str) -> Result<bool> {
    open(path)?
        .has(member)
        .with_context(|| format!("failed to inspect {member} in {path:?}"))
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
    (|| attributes(std::str::from_utf8(&bytes)?, DuplicateHeaders::Reject))()
        .with_context(|| format!("failed to parse META-INF/MANIFEST.MF in {path:?}"))
}

/// Main section only; continuation lines belong to their preceding attribute.
pub(crate) fn attributes(
    text: &str,
    duplicates: DuplicateHeaders,
) -> Result<HashMap<String, String>> {
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
        let previous = values.insert(key.to_ascii_lowercase(), value.to_owned());
        ensure!(
            matches!(duplicates, DuplicateHeaders::LastWins) || previous.is_none(),
            "duplicate manifest attribute {key}"
        );
    }
    Ok(values)
}

pub(crate) fn manifest_paths(
    jar: &Path,
    manifest: &HashMap<String, String>,
) -> Result<Vec<PathBuf>> {
    manifest
        .get("class-path")
        .map(|cp| {
            cp.split_whitespace()
                .map(|value| {
                    ensure!(
                        !value.contains(':') && !value.contains('%'),
                        "URI or encoded manifest classpaths are unsupported in {jar:?}!META-INF/MANIFEST.MF: {value:?}"
                    );
                    Ok(jar.parent().context("JAR path has no parent")?.join(value))
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}
