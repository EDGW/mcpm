//! Read-only projection of library observations; never parses storage JSON or identifies Mods.

use anyhow::{Context, Result};
use mcpm_lib::local::cache::{
    CacheInspection,
    hash::Blake3Hash,
    mods::{CacheStatus, ModsCacheDir, ident, meta},
};

use super::{ContentStatus, Entry, FileKind, Inspection, ListEntry, Metadata, ReadOptions};

/// Observes only requested data; shared by operations and the still-local refresh presentation.
pub fn observe(
    cache: &ModsCacheDir,
    id: Blake3Hash,
    inspection: Result<CacheInspection>,
    options: ReadOptions,
) -> Result<Entry> {
    let inspection: Inspection = inspection
        .with_context(|| format!("failed to inspect cached content {id}"))?
        .into();
    let mut entry = content_entry(id, cache.content_path(&id), inspection.content.as_ref());
    let decode = options.metadata && options.metadata_details;
    let mut meta = (options.sha256
        || (decode
            && inspection
                .metadata
                .iter()
                .any(|item| item.name == meta::NAME)))
    .then(|| cache.try_mod_metadata(id))
    .transpose()
    .with_context(|| format!("failed to observe Loader metadata for {id}"))?;
    entry.sha256 = meta
        .as_ref()
        .filter(|meta| meta.status() == &CacheStatus::Available)
        .and_then(|meta| meta.digest())
        .map(|digest| digest.sha256().to_owned());
    if options.metadata {
        for item in inspection.metadata {
            entry.metadatas.push(if !options.metadata_details {
                Metadata::Other(item.name)
            } else {
                match item.name.as_str() {
                    meta::NAME => {
                        Metadata::Meta((&meta.take().expect("requested Loader observation")).into())
                    }
                    ident::NAME => Metadata::Ident(
                        (&cache.try_ident_info(id).with_context(|| {
                            format!("failed to observe source identification for {id}")
                        })?)
                            .into(),
                    ),
                    _ => Metadata::Other(item.name),
                }
            });
        }
    }
    Ok(entry)
}

impl ListEntry {
    /// Builds a displayable structural observation without re-reading the filesystem.
    pub fn into_entry(self, include_metadata: bool) -> Result<Entry> {
        let inspection = self
            .inspection
            .map_err(|error| error.into_inner())
            .with_context(|| format!("failed to inspect cached content {}", self.key))?;
        let mut entry = content_entry(self.key, self.file, inspection.content.as_ref());
        if include_metadata {
            entry.metadatas = inspection
                .metadata
                .into_iter()
                .map(|item| Metadata::Other(item.name))
                .collect();
        }
        Ok(entry)
    }
}

fn content_entry(id: Blake3Hash, file: std::path::PathBuf, content: Option<&FileKind>) -> Entry {
    let (status, size, reason) = match content {
        Some(FileKind::File { size }) => (ContentStatus::Available, Some(*size), None),
        None => (ContentStatus::NotFound, None, None),
        Some(_) => (
            ContentStatus::Unavailable,
            None,
            Some("cached content is not a regular file".into()),
        ),
    };
    Entry {
        id,
        file,
        status,
        size,
        reason,
        sha256: None,
        metadatas: Vec::new(),
    }
}
