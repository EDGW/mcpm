//! Errors from explicit cache operations; availability probes use CacheStatus instead.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::{CacheId, CacheUnavailableReason};
use crate::local::{files::HardLinkError, mcmod::cached_meta::MetadataConversionError};

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cannot locate the user's home directory")]
    MissingHome,
    #[error("broken cache directory {path:?}: {reason}")]
    BrokenDirectory { path: PathBuf, reason: String },
    #[error("unsupported cache directory version {version} at {path:?}")]
    UnsupportedDirectoryVersion { path: PathBuf, version: u32 },
    #[error("export destination {0:?} must be outside the cache directory")]
    ExportIntoCache(PathBuf),
    #[error("cannot {operation} {path:?}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cache object {0} does not exist")]
    NotFound(CacheId),
    #[error("metadata source changed while it was being identified: {0:?}")]
    SourceChanged(PathBuf),
    #[error("cache object {id} is unavailable: {reason:?}")]
    Unavailable {
        id: CacheId,
        reason: CacheUnavailableReason,
    },
    #[error("cannot parse cache digest {path:?}: {source}")]
    Digest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cannot parse source identification {path:?}: {source}")]
    IdentificationData {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid source match: {0}")]
    InvalidSourceMatch(#[source] anyhow::Error),
    #[error("source {source_id:?} failed to identify cached file: {source}")]
    SourceIdentify {
        source_id: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("loader {loader:?} failed to identify cached mod: {source}")]
    Identify {
        loader: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("cache quick index failure: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("quick index is busy or its recovery lease failed: {0}")]
    IndexBusy(String),
    #[error("unsupported cache quick index version {0}; rebuild the index")]
    UnsupportedIndexVersion(i64),
    #[error(transparent)]
    MetadataConversion(#[from] MetadataConversionError),
    #[error(transparent)]
    HardLink(#[from] HardLinkError),
}

pub(super) trait IoContext<T> {
    fn at(self, operation: &'static str, path: &std::path::Path) -> Result<T, CacheError>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, operation: &'static str, path: &std::path::Path) -> Result<T, CacheError> {
        self.map_err(|source| CacheError::Io {
            operation,
            path: path.to_owned(),
            source,
        })
    }
}
