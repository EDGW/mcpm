//! Typed Loader snapshot observations and explicit verification over cache handles.

use super::super::status::{observation_error, unexpected_error};
use super::super::{CacheStatus, CacheUnavailableReason};
use super::{CacheDigest, codec::METANAME};
use crate::local::cache::{
    CacheContent, CacheDir, CacheFileKind, CacheInspection, VerificationMode, hash::Blake3Hash,
};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CacheEntry {
    id: Blake3Hash,
    data_path: PathBuf,
    digest_path: PathBuf,
    pub(super) status: CacheStatus,
    pub(super) digest: Option<CacheDigest>,
}
impl CacheEntry {
    fn new(cache: &CacheDir, id: Blake3Hash) -> Self {
        Self {
            id,
            data_path: cache.content_path(&id),
            digest_path: cache
                .metadata_file_path(&id, METANAME)
                .expect("fixed metadata name is valid"),
            status: CacheStatus::NotFound,
            digest: None,
        }
    }
    pub fn id(&self) -> Blake3Hash {
        self.id
    }
    pub fn data_path(&self) -> &Path {
        &self.data_path
    }
    /// Snapshot location for diagnostics; access metadata through `ModsCacheDir`.
    pub fn digest_path(&self) -> &Path {
        &self.digest_path
    }
    pub fn status(&self) -> &CacheStatus {
        &self.status
    }
    pub fn digest(&self) -> Option<&CacheDigest> {
        self.digest.as_ref()
    }
}

pub(super) fn read(cache: &CacheDir, key: Blake3Hash) -> CacheEntry {
    from_inspection(cache, key, cache.inspect(&key))
}

pub(super) fn try_read(cache: &CacheDir, key: Blake3Hash) -> Result<CacheEntry> {
    try_from_inspection(cache, key, cache.inspect(&key))
}

pub(super) fn from_inspection(
    cache: &CacheDir,
    key: Blake3Hash,
    inspection: Result<CacheInspection>,
) -> CacheEntry {
    try_from_inspection(cache, key, inspection).unwrap_or_else(|error| {
        let mut entry = CacheEntry::new(cache, key);
        entry.status = unexpected_error(error);
        entry
    })
}

fn try_from_inspection(
    cache: &CacheDir,
    key: Blake3Hash,
    inspection: Result<CacheInspection>,
) -> Result<CacheEntry> {
    let mut entry = CacheEntry::new(cache, key);
    let result: Result<CacheStatus> = (|| {
        let inspection = inspection?;
        let metadata = inspection
            .metadata
            .iter()
            .find(|item| item.name == METANAME);
        let Some(kind) = inspection.content else {
            return Ok(if metadata.is_none() {
                CacheStatus::NotFound
            } else {
                invalid()
            });
        };
        if !matches!(kind, CacheFileKind::File { .. }) {
            return Ok(invalid());
        }
        let Some(metadata) = metadata else {
            return Ok(CacheStatus::Unavailable(
                CacheUnavailableReason::MissingDigest,
            ));
        };
        if !matches!(metadata.kind, CacheFileKind::File { .. }) {
            return Ok(invalid());
        }
        let content = cache
            .open_content_with_mode(&key, VerificationMode::Structure)?
            .context("cached content disappeared during metadata observation")?;
        let observed = try_from_content(cache, &content)?;
        entry.digest = observed.digest;
        Ok(observed.status)
    })();
    entry.status = result.or_else(observation_error)?;
    Ok(entry)
}

/// Reuses a pinned lookup result without rehashing immutable contents.
pub fn from_content(cache: &CacheDir, content: &CacheContent) -> CacheEntry {
    try_from_content(cache, content).unwrap_or_else(|error| {
        let mut entry = CacheEntry::new(cache, content.key());
        entry.status = unexpected_error(error);
        entry
    })
}

fn try_from_content(cache: &CacheDir, content: &CacheContent) -> Result<CacheEntry> {
    let key = content.key();
    let mut entry = CacheEntry::new(cache, key);
    let result: Result<CacheStatus> = (|| {
        let Some(digest) = content.try_read_metadata::<CacheDigest>(METANAME)? else {
            return Ok(CacheStatus::Unavailable(
                CacheUnavailableReason::MissingDigest,
            ));
        };
        let valid = digest.blake3 == key
            && digest.size == content.size()
            && digest.valid_content_descriptor()
            && digest.valid_metadata();
        entry.digest = Some(digest);
        Ok(if valid {
            CacheStatus::Available
        } else {
            invalid()
        })
    })();
    entry.status = result.or_else(observation_error)?;
    Ok(entry)
}

pub(super) fn sha256(content: &CacheContent) -> Result<String> {
    use std::io::Seek;
    let mut file = content.file();
    file.rewind()
        .context("failed to rewind cached content for SHA-256")?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).context("failed to hash cached content with SHA-256")?;
    content.validate()?;
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn verify(cache: &CacheDir, key: Blake3Hash, mode: VerificationMode) -> CacheStatus {
    let entry = read(cache, key);
    if mode == VerificationMode::Structure {
        return entry.status;
    }
    if !matches!(
        entry.status,
        CacheStatus::Available | CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    ) {
        return entry.status;
    }
    let result: Result<CacheStatus> = (|| {
        let content = cache
            .open_content(&key)?
            .context("cached content disappeared during verification")?;
        if let Some(digest) = entry.digest.as_ref() {
            if sha256(&content)? != digest.sha256 {
                return Ok(invalid());
            }
        }
        Ok(entry.status)
    })();
    result
        .or_else(observation_error)
        .unwrap_or_else(unexpected_error)
}

pub(super) fn invalid() -> CacheStatus {
    CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
}
