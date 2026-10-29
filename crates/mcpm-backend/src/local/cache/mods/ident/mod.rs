//! Source identification policy and typed snapshots over the generic content cache.

use super::status::{observation_error, unexpected_error};
use super::{CacheStatus, CacheUnavailableReason, MetadataRefresh};
use crate::local::cache::{
    CacheContent, CacheDir, CacheFileKind, VerificationMode, hash::Blake3Hash,
};
use crate::source::{FileMatch, FileSummary, Source};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod codec;
pub use codec::METANAME as NAME;
use codec::{METANAME, Snapshot};

#[derive(Debug, Clone)]
pub struct CacheIdentInfo {
    id: Blake3Hash,
    path: PathBuf,
    status: CacheStatus,
    snapshot: Option<Snapshot>,
}
impl CacheIdentInfo {
    pub fn id(&self) -> Blake3Hash {
        self.id
    }
    /// Snapshot location for diagnostics; access metadata through `ModsCacheDir`.
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn status(&self) -> &CacheStatus {
        &self.status
    }
    pub fn matches(&self) -> impl Iterator<Item = (&str, &FileMatch)> {
        self.snapshot
            .iter()
            .filter(|_| self.status == CacheStatus::Available)
            .flat_map(|snapshot| {
                snapshot
                    .sources
                    .iter()
                    .map(|(key, file)| (key.as_str(), file))
            })
    }
    pub fn get(&self, source: &str) -> Option<&FileMatch> {
        self.matches()
            .find_map(|(key, file)| (key == source).then_some(file))
    }
    pub fn verified_matches(&self, summary: &FileSummary) -> BTreeMap<String, FileMatch> {
        self.matches()
            .filter(|(_, file)| file.validate(summary).is_ok())
            .map(|(key, file)| (key.to_owned(), file.clone()))
            .collect()
    }
}

/// One source's refresh result and the resulting stored snapshot observation.
/// Refresh status is transient; only verified successful matches are persisted.
#[derive(Debug, Clone)]
pub struct RefreshOutcome {
    pub status: RefreshStatus,
    pub info: CacheIdentInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshStatus {
    /// Reused an existing verified source match without a provider request.
    Reused,
    /// The provider returned a verified match, which was committed to the cache.
    Matched,
    /// The provider returned no match; existing positive records were preserved.
    Unmatched,
}

impl super::ModsCacheDir {
    pub fn ident_info(&self, key: Blake3Hash) -> CacheIdentInfo {
        read(&self.cache, key)
    }
    /// Observes source metadata, preserving unexpected I/O errors for diagnostics.
    /// Missing or invalid snapshots remain ordinary availability states.
    pub fn try_ident_info(&self, key: Blake3Hash) -> Result<CacheIdentInfo> {
        try_read(&self.cache, key)
    }
    pub fn write_ident_info(&self, key: Blake3Hash, matched: &FileMatch) -> Result<CacheIdentInfo> {
        write(&self.cache, key, matched)
    }
    /// Refreshes one source and returns the stored snapshot, which contains only matches.
    /// Use `refresh_ident_info_with_outcome` to distinguish reuse, matching and no match.
    pub async fn refresh_ident_info(
        &self,
        key: Blake3Hash,
        source: &dyn Source,
        mode: MetadataRefresh,
    ) -> Result<CacheIdentInfo> {
        Ok(self
            .refresh_ident_info_with_outcome(key, source, mode)
            .await?
            .info)
    }
    /// Reports the current operation separately from the stored positive matches.
    /// An unmatched result neither creates a snapshot nor removes earlier matches.
    pub async fn refresh_ident_info_with_outcome(
        &self,
        key: Blake3Hash,
        source: &dyn Source,
        mode: MetadataRefresh,
    ) -> Result<RefreshOutcome> {
        refresh(&self.cache, key, source, mode).await
    }
    pub fn find_ident_info(&self, path: &Path) -> Result<Option<CacheIdentInfo>> {
        find(&self.cache, path)
    }
}

fn empty(cache: &CacheDir, key: Blake3Hash) -> CacheIdentInfo {
    CacheIdentInfo {
        id: key,
        path: cache
            .metadata_file_path(&key, METANAME)
            .expect("fixed metadata name is valid"),
        status: CacheStatus::NotFound,
        snapshot: None,
    }
}

fn invalid() -> CacheStatus {
    CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
}

fn read(cache: &CacheDir, key: Blake3Hash) -> CacheIdentInfo {
    try_read(cache, key).unwrap_or_else(|error| {
        let mut info = empty(cache, key);
        info.status = unexpected_error(error);
        info
    })
}

fn try_read(cache: &CacheDir, key: Blake3Hash) -> Result<CacheIdentInfo> {
    let mut info = empty(cache, key);
    let result: Result<CacheStatus> = (|| {
        let inspection = cache.inspect(&key)?;
        let metadata = inspection
            .metadata
            .iter()
            .find(|meta| meta.name == METANAME);
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
        let content = cache
            .open_content_with_mode(&key, VerificationMode::Structure)?
            .context("cached content disappeared during source observation")?;
        let observed = try_from_content(cache, &content)?;
        info.snapshot = observed.snapshot;
        Ok(observed.status)
    })();
    info.status = result.or_else(observation_error)?;
    Ok(info)
}

/// Reuses a generic read-only lookup without an extra full-content hash.
fn from_content(cache: &CacheDir, content: &CacheContent) -> CacheIdentInfo {
    try_from_content(cache, content).unwrap_or_else(|error| {
        let mut info = empty(cache, content.key());
        info.status = unexpected_error(error);
        info
    })
}

fn try_from_content(cache: &CacheDir, content: &CacheContent) -> Result<CacheIdentInfo> {
    let mut info = empty(cache, content.key());
    let result: Result<CacheStatus> = (|| {
        let Some(snapshot) = content.try_read_metadata::<Snapshot>(METANAME)? else {
            return Ok(CacheStatus::Unavailable(
                CacheUnavailableReason::MissingDigest,
            ));
        };
        if !snapshot.valid(content.key(), content.size()) {
            return Ok(invalid());
        }
        info.snapshot = Some(snapshot);
        Ok(CacheStatus::Available)
    })();
    info.status = result.or_else(observation_error)?;
    Ok(info)
}

/// Looks up only source metadata for the queried file, without cache writes.
pub(super) fn find(cache: &CacheDir, file: &Path) -> Result<Option<CacheIdentInfo>> {
    match cache.find_content(file) {
        Ok(content) => Ok(content.map(|content| from_content(cache, &content))),
        Err(error) => {
            if let Some(mismatch) =
                error.downcast_ref::<crate::local::cache::CachedContentLookupError>()
            {
                let mut info = empty(cache, mismatch.key);
                info.status = observation_error(error).unwrap_or_else(unexpected_error);
                Ok(Some(info))
            } else {
                Err(error)
            }
        }
    }
}

fn write(cache: &CacheDir, key: Blake3Hash, matched: &FileMatch) -> Result<CacheIdentInfo> {
    let source_key = &matched.reference.project.source.0;
    ensure!(
        !source_key.is_empty() && !source_key.contains('@'),
        "invalid source cache identity"
    );
    let content = cache
        .open_content(&key)?
        .context("source identification requires cached content")?;
    let summary = FileSummary::read(content.path(), Default::default())
        .context("failed to summarize cached source content")?;
    content.validate()?;
    matched.validate(&summary)?;
    let existing = from_content(cache, &content);
    if existing
        .get(source_key)
        .is_some_and(|saved| saved == matched)
    {
        return Ok(existing);
    }
    let mut snapshot = existing
        .snapshot
        .filter(|saved| {
            saved.valid(key, summary.size) && saved.sha1.eq_ignore_ascii_case(&summary.sha1)
        })
        .unwrap_or_else(|| Snapshot::new(key, &summary));
    snapshot.sources.insert(source_key.clone(), matched.clone());
    content.validate()?;
    cache
        .write_metadata(&key, METANAME, &snapshot)?
        .context("content disappeared during source metadata commit")?;
    content.validate()?;
    Ok(read(cache, key))
}

async fn refresh(
    cache: &CacheDir,
    key: Blake3Hash,
    source: &dyn Source,
    mode: MetadataRefresh,
) -> Result<RefreshOutcome> {
    let source_key = source.identification_cache_id();
    ensure!(
        !source_key.0.is_empty() && !source_key.0.contains('@'),
        "invalid source cache identity"
    );
    let content = cache
        .open_content(&key)?
        .context("source refresh requires cached content")?;
    let summary = FileSummary::read(content.path(), source.summary_requirements())
        .context("failed to summarize cached source content")?;
    content.validate()?;
    let existing = from_content(cache, &content);
    if mode == MetadataRefresh::MissingOrOutdated
        && existing
            .get(&source_key.0)
            .is_some_and(|file| file.validate(&summary).is_ok())
    {
        return Ok(RefreshOutcome {
            status: RefreshStatus::Reused,
            info: existing,
        });
    }
    let matched = source.identify_summary(&summary).await.with_context(|| {
        format!(
            "source {:?} failed to identify cached content {key}",
            source_key.0
        )
    })?;
    content.validate()?;
    let Some(matched) = matched else {
        return Ok(RefreshOutcome {
            status: RefreshStatus::Unmatched,
            info: read(cache, key),
        });
    };
    matched.validate(&summary)?;
    let matched = matched.for_cache(source)?;
    // Publish the trusted successful result through the same byte/provenance checks.
    Ok(RefreshOutcome {
        status: RefreshStatus::Matched,
        info: write(cache, key, &matched)?,
    })
}
