//! Content inventory, structural diagnostics and removal of owned cache names.

use super::{CacheDir, hash::Blake3Hash, index, layout, storage};
use anyhow::{Context, Result};
use std::{collections::BTreeSet, fs, io};

impl CacheDir {
    /// Observes one regular content file and its sorted regular metadata names,
    /// without the storage layer's `.json` suffix.
    /// Missing content, symbolic links and non-regular objects return `None`.
    /// Does not open or hash content; names may change after this observation.
    pub fn entry(&self, key: &Blake3Hash) -> Result<Option<CacheEntry>> {
        let inspection = self.inspect(key)?;
        if !matches!(inspection.content, Some(CacheFileKind::File { .. })) {
            return Ok(None);
        }
        let metanames = inspection
            .metadata
            .into_iter()
            .filter(|entry| matches!(entry.kind, CacheFileKind::File { .. }))
            .map(|entry| entry.name)
            .collect();
        Ok(Some(CacheEntry {
            blake3: *key,
            metanames,
        }))
    }

    /// Lists regular content keys in canonical shards, sorted by hash.
    /// Ignores metadata, temporary names, symbolic links and misplaced objects.
    /// Does not verify content and is not a filesystem snapshot.
    pub fn list(&self) -> Result<Vec<Blake3Hash>> {
        self.named_keys(true)
    }

    /// Observes structure without reading file contents or decoding metadata.
    /// Includes non-regular content and sidecars, even when the content is absent.
    pub fn inspect(&self, key: &Blake3Hash) -> Result<CacheInspection> {
        let path = self.data_path(key);
        let parent = path.parent().expect("cache object has a shard");
        match fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(CacheInspection::default()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(CacheInspection::default());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect cache shard {parent:?}"));
            }
        }
        let entries = match fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(CacheInspection::default());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read cache shard {parent:?}"));
            }
        };
        let filename = key.to_string();
        let prefix = format!("{filename}.");
        let mut inspection = CacheInspection::default();
        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read cache shard entry {parent:?}"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let suffix = name
                .strip_prefix(&prefix)
                .and_then(layout::metaname_from_suffix);
            if name != filename && suffix.is_none() {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect cache object {:?}", entry.path())
                    });
                }
            };
            let kind = CacheFileKind::from_metadata(&metadata);
            if let Some(name) = suffix {
                inspection.metadata.push(CacheMetadataEntry {
                    name: name.to_owned(),
                    kind,
                });
            } else {
                inspection.content = Some(kind);
            }
        }
        inspection
            .metadata
            .sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(inspection)
    }

    /// Scans owned names, including orphan metadata and invalid object types.
    /// Enumeration errors fail the scan; individual inspection errors retain their keys.
    pub fn scan(&self) -> Result<Vec<CacheScanEntry>> {
        Ok(self
            .named_keys(false)?
            .into_iter()
            .map(|key| CacheScanEntry {
                key,
                inspection: self.inspect(&key),
            })
            .collect())
    }

    /// Removes content and all owned metadata names, including orphan snapshots.
    /// Other hard links survive. Unknown files and directories are never recursively
    /// removed; failures can leave a partially removed object. Concurrent publishers
    /// can recreate names after removal.
    pub fn remove(&self, key: &Blake3Hash) -> Result<bool> {
        if !layout::shard_exists(&self.path, key)? {
            index::forget(&self.path, key);
            return Ok(false);
        }
        let path = self.data_path(key);
        let parent = path.parent().expect("cache object has a shard");
        let mut paths = vec![path.clone()];
        for entry in fs::read_dir(parent)
            .with_context(|| format!("failed to enumerate cache removal {parent:?}"))?
        {
            let entry = entry.context("failed to read cache removal entry")?;
            if entry.path() != path
                && layout::key_from_path(&self.path, &entry.path()) == Some(*key)
            {
                paths.push(entry.path());
            }
        }
        let mut removed = false;
        for path in paths {
            removed |= storage::remove_if_exists(&path)?;
        }
        index::forget(&self.path, key);
        Ok(removed)
    }
    /// Shared name enumeration for regular content and diagnostic inventory.
    fn named_keys(&self, content_only: bool) -> Result<Vec<Blake3Hash>> {
        let mut keys = BTreeSet::new();
        for shard in fs::read_dir(&self.path)
            .with_context(|| format!("failed to scan cache directory {:?}", self.path))?
        {
            let shard = shard.context("failed to read cache shard entry")?;
            if !shard
                .file_name()
                .to_str()
                .is_some_and(layout::valid_shard_name)
            {
                continue;
            }
            if !shard
                .file_type()
                .with_context(|| format!("failed to inspect shard {:?}", shard.path()))?
                .is_dir()
            {
                continue;
            }
            let entries = match fs::read_dir(shard.path()) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to scan shard {:?}", shard.path()));
                }
            };
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!("failed to read cache object in {:?}", shard.path())
                })?;
                let Some(key) = layout::key_from_path(&self.path, &entry.path()) else {
                    continue;
                };
                if content_only
                    && (entry.path() != self.data_path(&key)
                        || !entry
                            .file_type()
                            .with_context(|| {
                                format!("failed to inspect content {:?}", entry.path())
                            })?
                            .is_file())
                {
                    continue;
                }
                keys.insert(key);
            }
        }
        Ok(keys.into_iter().collect())
    }
}

/// A directory observation, independent of metadata decoding and content verification.
#[derive(Debug)]
pub struct CacheEntry {
    pub blake3: Blake3Hash,
    /// Plain metadata names, without the storage layer's `.json` suffix.
    pub metanames: Vec<String>,
}

/// Structural observations; a regular file has not necessarily passed a hash check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheFileKind {
    File { size: u64 },
    Directory,
    SymbolicLink,
    Other,
}

impl CacheFileKind {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        if metadata.is_file() {
            Self::File {
                size: metadata.len(),
            }
        } else if metadata.is_dir() {
            Self::Directory
        } else if metadata.file_type().is_symlink() {
            Self::SymbolicLink
        } else {
            Self::Other
        }
    }
}

#[derive(Debug)]
pub struct CacheMetadataEntry {
    /// Plain metadata name, without the storage layer's `.json` suffix.
    pub name: String,
    pub kind: CacheFileKind,
}

#[derive(Debug, Default)]
pub struct CacheInspection {
    pub content: Option<CacheFileKind>,
    pub metadata: Vec<CacheMetadataEntry>,
}

#[derive(Debug)]
pub struct CacheScanEntry {
    pub key: Blake3Hash,
    pub inspection: Result<CacheInspection>,
}
