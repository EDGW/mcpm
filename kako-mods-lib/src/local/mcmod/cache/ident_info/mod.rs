//! Independent source-identification observations and verified refresh over cached bytes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{
    CacheError, CacheId, CacheStatus, CacheUnavailableReason, MetadataRefresh,
    content::ContentFile, operations::Store, storage,
};
use crate::source::{FileMatch, FileSummary, Source};

mod codec;
use codec::Snapshot;

/// Source matches are available independently of loader metadata, but require .dat.
#[derive(Debug, Clone)]
pub struct CacheIdentInfo {
    pub(super) id: CacheId,
    pub(super) path: PathBuf,
    pub(super) status: CacheStatus,
    snapshot: Option<Snapshot>,
}
impl CacheIdentInfo {
    pub fn id(&self) -> CacheId {
        self.id
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub fn status(&self) -> &CacheStatus {
        &self.status
    }
    /// Only valid observations expose reusable results. A missing platform is unchecked.
    pub fn matches(&self) -> impl Iterator<Item = (&str, &FileMatch)> {
        self.snapshot
            .iter()
            .filter(|_| self.status == CacheStatus::Available)
            .flat_map(|snapshot| {
                snapshot
                    .sources
                    .iter()
                    .map(|(source, file)| (source.as_str(), file))
            })
    }
    pub fn get(&self, source: &str) -> Option<&FileMatch> {
        self.matches()
            .find_map(|(key, file)| (key == source).then_some(file))
    }
    /// Validates matches against the actual queried file before they cross a transport boundary.
    pub fn verified_matches(&self, summary: &FileSummary) -> BTreeMap<String, FileMatch> {
        self.matches()
            .filter(|(_, file)| file.validate(summary).is_ok())
            .map(|(key, file)| (key.to_owned(), file.clone()))
            .collect()
    }
}

impl Store {
    pub(super) fn ident_path(&self, id: CacheId) -> PathBuf {
        let id = id.to_string();
        self.root
            .join(&id[..2])
            .join(format!("{id}.identification.json"))
    }

    pub(super) fn ident_info(&self, id: CacheId) -> CacheIdentInfo {
        use super::error::IoContext;
        let (data, _) = self.paths(id);
        let path = self.ident_path(id);
        let mut entry = CacheIdentInfo {
            id,
            path,
            status: CacheStatus::NotFound,
            snapshot: None,
        };
        let result = (|| {
            let data = storage::exists(&data).at("inspect cache data", &data)?;
            let info =
                storage::exists(&entry.path).at("inspect source identification", &entry.path)?;
            let Some(data) = data else {
                return Ok(if info.is_none() {
                    CacheStatus::NotFound
                } else {
                    invalid_status()
                });
            };
            if !data.is_file() {
                return Ok(invalid_status());
            }
            let Some(info) = info else {
                return Ok(CacheStatus::Unavailable(
                    CacheUnavailableReason::MissingDigest,
                ));
            };
            if !info.is_file() {
                return Ok(invalid_status());
            }
            let snapshot = match codec::read(&entry.path) {
                Ok(snapshot) => snapshot,
                Err(CacheError::IdentificationData { .. }) => return Ok(invalid_status()),
                Err(error) => return Err(error),
            };
            if !snapshot.valid(id, data.len()) {
                return Ok(invalid_status());
            }
            entry.snapshot = Some(snapshot);
            Ok::<_, CacheError>(CacheStatus::Available)
        })();
        entry.status = result.unwrap_or_else(|error| {
            CacheStatus::Unavailable(CacheUnavailableReason::UnexpectedError(error.to_string()))
        });
        entry
    }

    /// Accepts only a verified match for this exact stored content, never a missing .dat.
    pub(super) fn write_ident_info(
        &self,
        id: CacheId,
        matched: &FileMatch,
    ) -> Result<CacheIdentInfo, CacheError> {
        let key = &matched.reference.project.source.0;
        if key.is_empty() || key.contains('@') {
            return Err(CacheError::InvalidSourceMatch(anyhow::anyhow!(
                "invalid source cache identity"
            )));
        }
        let (path, _) = self.paths(id);
        let mut content = ContentFile::open(&path)?;
        if content.id()? != id {
            return Err(Self::invalid(id));
        }
        let summary =
            FileSummary::read(&path, Default::default()).map_err(CacheError::InvalidSourceMatch)?;
        matched
            .validate(&summary)
            .map_err(CacheError::InvalidSourceMatch)?;
        let existing = self.ident_info(id);
        if existing.get(key).is_some_and(|saved| saved == matched) {
            content.validate()?;
            return Ok(existing);
        }
        let mut snapshot = existing
            .snapshot
            .filter(|snapshot| {
                snapshot.valid(id, summary.size)
                    && snapshot.sha1.eq_ignore_ascii_case(&summary.sha1)
            })
            .unwrap_or_else(|| Snapshot::new(id, &summary));
        snapshot.sources.insert(key.clone(), matched.clone());
        content.validate()?;
        codec::write(&self.ident_path(id), &snapshot)?;
        Ok(self.ident_info(id))
    }

    /// Identifies immutable .dat bytes; neither the loader descriptor nor a caller's metadata is trusted.
    pub(super) async fn refresh_ident_info(
        &self,
        id: CacheId,
        source: &dyn Source,
        mode: MetadataRefresh,
    ) -> Result<CacheIdentInfo, CacheError> {
        let key = source.identification_cache_id();
        if key.0.is_empty() || key.0.contains('@') {
            return Err(CacheError::InvalidSourceMatch(anyhow::anyhow!(
                "cache source identity must be nonempty and contain no credentials"
            )));
        }
        let (path, _) = self.paths(id);
        let mut content = ContentFile::open(&path)?;
        if content.id()? != id {
            return Err(Self::invalid(id));
        }
        let summary = FileSummary::read(&path, source.summary_requirements())
            .map_err(CacheError::InvalidSourceMatch)?;
        content.validate()?;
        let existing = self.ident_info(id);
        if mode == MetadataRefresh::MissingOrOutdated
            && existing
                .get(&key.0)
                .is_some_and(|file| file.validate(&summary).is_ok())
        {
            return Ok(existing);
        }
        let matched = source
            .identify_summary(&summary)
            .await
            .map_err(|source_error| CacheError::SourceIdentify {
                source_id: key.0.clone(),
                source: source_error,
            })?;
        content.validate()?;
        let Some(mut matched) = matched else {
            return Ok(self.ident_info(id));
        };
        matched
            .validate(&summary)
            .map_err(CacheError::InvalidSourceMatch)?;
        matched = matched
            .for_cache(source)
            .map_err(CacheError::InvalidSourceMatch)?;
        let mut snapshot = self
            .ident_info(id)
            .snapshot
            .filter(|snapshot| {
                snapshot.valid(id, summary.size)
                    && snapshot.sha1.eq_ignore_ascii_case(&summary.sha1)
            })
            .unwrap_or_else(|| Snapshot::new(id, &summary));
        snapshot.sources.insert(key.0, matched);
        content.validate()?;
        codec::write(&self.ident_path(id), &snapshot)?;
        Ok(self.ident_info(id))
    }
}
fn invalid_status() -> CacheStatus {
    CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
}
