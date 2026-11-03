//! Shared observations of typed cache metadata, independent of business implementations.

use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStatus {
    Available,
    Unavailable(CacheUnavailableReason),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheUnavailableReason {
    /// Content exists, but the requested metadata snapshot is absent.
    MissingDigest,
    /// Invalid content association, unsupported metadata or failed verification.
    InvalidData,
    UnexpectedError(String),
}

/// Unusable snapshots are states; unexpected filesystem failures retain their cause chain.
pub(super) fn observation_error(error: anyhow::Error) -> Result<CacheStatus> {
    if error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() != std::io::ErrorKind::NotFound)
    }) {
        Err(error)
    } else {
        Ok(CacheStatus::Unavailable(
            CacheUnavailableReason::InvalidData,
        ))
    }
}

/// Compatibility for callers that request a status-only observation.
pub(super) fn unexpected_error(error: anyhow::Error) -> CacheStatus {
    CacheStatus::Unavailable(CacheUnavailableReason::UnexpectedError(format!(
        "{error:#}"
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataRefresh {
    MissingOrOutdated,
    Force,
}
