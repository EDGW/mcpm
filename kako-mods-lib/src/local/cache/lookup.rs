//! Read-only file lookup by sampled candidates, pinned identities and BLAKE3 fallback.

use super::{
    CacheContent, CacheDir, ContentVerification,
    hash::{Blake3Hash, ImoHash},
    index,
    storage::Content,
};
use anyhow::{Context, Result};
use std::path::Path;

impl CacheDir {
    /// Finds content without importing files, changing permissions or writing index state.
    /// imohash selects candidates only; matching pinned identities establish the
    /// immutable hard-link fast path. Independent copies use full BLAKE3 verification.
    pub fn find_content(&self, file: &Path) -> Result<Option<CacheContent>> {
        let mut source = Content::open(file)?
            .with_context(|| format!("lookup input does not exist: {file:?}"))?;
        let quick = source.quick_hash()?;
        let candidates = index::candidates(&self.path, &quick).unwrap_or_else(|error| {
            tracing::debug!(error = %format!("{error:#}"), "Quick index unavailable; using full BLAKE3 lookup");
            Vec::new()
        });
        for key in candidates {
            let candidate = match self.open_raw(&key) {
                Ok(Some(candidate)) => candidate,
                Ok(None) => continue,
                Err(error) => {
                    tracing::debug!(%key, error = %format!("{error:#}"), "Skipping unusable quick-index candidate");
                    continue;
                }
            };
            if candidate.identity() == source.identity() && candidate.validate().is_ok() {
                source.validate()?;
                return Ok(Some(CacheContent::new(
                    self.path.clone(),
                    key,
                    candidate,
                    ContentVerification::FileIdentity,
                )));
            }
        }
        let key = source.hash()?;
        let matched = self
            .open_content(&key)
            .map_err(|source| CachedContentLookupError { key, source })?;
        source.validate()?;
        Ok(matched)
    }

    /// Finds a canonical key using the same read-only lookup as `find_content`.
    pub fn find_file(&self, file: &Path) -> Result<Option<Blake3Hash>> {
        Ok(self.find_content(file)?.map(|content| content.key()))
    }

    /// Rebuilds the derived index from verified immutable content. Bad objects are
    /// skipped; concurrent imports/removals can leave missing or stale candidates.
    /// Recovery fails promptly while another cooperating index connection is active.
    pub fn rebuild_index(&self) -> Result<usize> {
        let mut entries = Vec::new();
        for key in self.list()? {
            let result: Result<Option<(ImoHash, Blake3Hash)>> = (|| {
                let Some(mut content) = self.open_verified(&key)? else {
                    return Ok(None);
                };
                Ok(Some((content.quick_hash()?, key)))
            })();
            match result {
                Ok(Some(entry)) => entries.push(entry),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%key, error = %format!("{error:#}"), "Skipping invalid content during index rebuild")
                }
            }
        }
        index::replace(&self.path, &entries)?;
        Ok(entries.len())
    }
}

/// The input was hashed successfully, but its matching cached object could not be
/// opened or verified. Business adapters can fall back without hiding input errors.
#[derive(Debug, thiserror::Error)]
#[error("cannot access matching cached content {key}: {source:#}")]
pub struct CachedContentLookupError {
    pub key: Blake3Hash,
    #[source]
    pub source: anyhow::Error,
}
