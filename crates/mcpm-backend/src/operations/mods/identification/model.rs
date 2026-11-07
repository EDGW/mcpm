//! Per-source identification facts and the sole derivation of file status and counters.

use anyhow::{Context, Result};
use mcpm_exec::error::PortableError;
use mcpm_lib::source::{FileMatch, SourceId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Final health of one file's requested source set, independent of publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryStatus {
    Success,
    PartialFailed,
    Failed,
}
impl QueryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::PartialFailed => "partial_failed",
            Self::Failed => "failed",
        }
    }
}

/// Effective reuse in this query, not the presence of old data on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheCoverage {
    All,
    Partial,
    None,
}
impl CacheCoverage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

/// Cache hits survive independent query failures; skipped work is never a fake request failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceResolution {
    Cached(FileMatch),
    Queried(Option<FileMatch>),
    Failed(Arc<PortableError>),
    Skipped,
}

/// Safe source identity and its final resolution, without source configuration or credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResult {
    pub source: SourceId,
    pub cache_id: Option<SourceId>,
    pub resolution: SourceResolution,
}
impl SourceResult {
    pub fn file(&self) -> Option<&FileMatch> {
        match &self.resolution {
            SourceResolution::Cached(file) | SourceResolution::Queried(Some(file)) => Some(file),
            _ => None,
        }
    }
    pub fn origin(&self) -> Option<&'static str> {
        match self.resolution {
            SourceResolution::Cached(_) => Some("cache"),
            SourceResolution::Queried(_) | SourceResolution::Failed(_) => Some("query"),
            SourceResolution::Skipped => None,
        }
    }
    pub fn match_status(&self) -> &'static str {
        match self.resolution {
            SourceResolution::Cached(_) | SourceResolution::Queried(Some(_)) => "matched",
            SourceResolution::Queried(None) => "unmatched",
            SourceResolution::Failed(_) => "failed",
            SourceResolution::Skipped => "skipped",
        }
    }
}

/// Completed source facts for one physical file; no independently mutable success flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub sources: Vec<SourceResult>,
}
impl QueryResult {
    pub fn status(&self) -> QueryStatus {
        if !self.has_failures() {
            QueryStatus::Success
        } else if self
            .sources
            .iter()
            .any(|source| matches!(source.resolution, SourceResolution::Cached(_)))
        {
            QueryStatus::PartialFailed
        } else {
            QueryStatus::Failed
        }
    }
    pub fn cache_coverage(&self) -> CacheCoverage {
        let cached = self
            .sources
            .iter()
            .filter(|source| matches!(source.resolution, SourceResolution::Cached(_)))
            .count();
        if cached == 0 {
            CacheCoverage::None
        } else if cached == self.sources.len() {
            CacheCoverage::All
        } else {
            CacheCoverage::Partial
        }
    }
    pub fn errors(&self) -> impl Iterator<Item = &PortableError> {
        self.sources
            .iter()
            .filter_map(|source| match &source.resolution {
                SourceResolution::Failed(error) => Some(error.as_ref()),
                _ => None,
            })
    }
    pub fn has_failures(&self) -> bool {
        self.errors().next().is_some()
    }
    /// Only new positive queries need publishing; reuse and unmatched never write.
    pub(crate) fn new_matches(&self) -> Vec<(SourceId, Result<FileMatch>)> {
        self.sources
            .iter()
            .filter_map(|source| match &source.resolution {
                SourceResolution::Queried(Some(file)) => Some((source, file)),
                _ => None,
            })
            .map(|(source, file)| {
                let matched = source
                    .cache_id
                    .as_ref()
                    .context("queried source has no cache identity")
                    .and_then(|cache_id| file.for_cache(&source.source, cache_id));
                (source.source.clone(), matched)
            })
            .collect()
    }
}

/// Unique physical file outcomes; preparation and selection failures remain distinct.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuerySummary {
    pub total: usize,
    pub success: usize,
    pub partial_failed: usize,
    pub failed: usize,
    pub preparation_failed: usize,
    pub selection_failed: usize,
}
impl QuerySummary {
    pub fn record(&mut self, result: &QueryResult) {
        match result.status() {
            QueryStatus::Success => self.success += 1,
            QueryStatus::PartialFailed => self.partial_failed += 1,
            QueryStatus::Failed => self.failed += 1,
        }
    }
    pub fn completed(&self) -> usize {
        self.success + self.partial_failed + self.failed
    }
    pub fn has_failures(&self) -> bool {
        self.partial_failed + self.failed + self.preparation_failed + self.selection_failed != 0
    }
}
