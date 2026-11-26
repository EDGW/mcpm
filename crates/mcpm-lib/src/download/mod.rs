//! Ordered URL translation and verified HTTP downloads, independent of download targets.
//!
//! Target wrappers own endpoint construction and metadata interpretation. This
//! module only routes requests and transfers files. Tokio I/O and time drivers
//! must be enabled by the caller.

use anyhow::{Context, Result, anyhow, ensure};
use url::Url;

pub mod authlib_injector;
pub mod catalog;
pub mod eula;
pub mod loader;
pub mod mojang;
pub mod sources;

pub use catalog::{Catalog, CatalogEndpoint};

mod client;
pub use client::{DownloadOptions, DownloadOutcome, Downloader};
mod config;
pub use config::DownloadConfig;
mod integrity;
pub use integrity::{Checksum, ChecksumAlgorithm, ChecksumMismatch, DownloadRequest, SizeMismatch};
mod source;
pub use source::{DownloadSource, MappingTable, UrlMapping};
mod progress;
pub use progress::{Event, RequestKind};

#[cfg(test)]
mod tests;

/// Appends validated URL path segments onto `base`, rejecting traversal,
/// path separators, query/fragment delimiters, whitespace, and control characters.
pub(crate) fn append_path_segments(base: &str, segments: &[&str], label: &str) -> Result<Url> {
    let mut url = Url::parse(base).with_context(|| format!("invalid base URL {base:?}"))?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow!("cannot append path segments to {base}"))?;
        path.pop_if_empty();
        for segment in segments {
            ensure!(is_valid_segment(segment), "invalid {label}: {segment:?}");
            path.push(segment);
        }
    }
    Ok(url)
}

fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '/' | '\\' | '?' | '#'))
}
