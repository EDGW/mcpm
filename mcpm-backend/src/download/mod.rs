//! Ordered URL translation and verified HTTP downloads, independent of download targets.
//!
//! Target wrappers own endpoint construction and metadata interpretation. This
//! module only routes requests and transfers files. Tokio I/O and time drivers
//! must be enabled by the caller.

pub mod authlib_injector;
pub mod sources;

mod client;
pub use client::{DownloadOptions, DownloadOutcome, Downloader};
mod config;
pub use config::DownloadConfig;
mod integrity;
pub use integrity::{Checksum, ChecksumAlgorithm, ChecksumMismatch, DownloadRequest, SizeMismatch};
mod source;
pub use source::DownloadSource;
mod progress;
pub use progress::{Event, RequestKind};

#[cfg(test)]
mod tests;
