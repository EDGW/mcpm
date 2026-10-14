use std::path::PathBuf;

use thiserror::Error;

/// Errors reported by the Quilt identification boundary.
#[derive(Debug, Error)]
pub enum QuiltError {
    /// The archive could not be opened or decoded as ZIP.
    #[error("Quilt: failed to open mod archive {path:?}")]
    OpenArchive {
        /// The mod file's path.
        path: PathBuf,
        /// The underlying filesystem or archive error.
        #[source]
        source: anyhow::Error,
    },
    /// A recognized metadata document could not be read.
    #[error("Quilt: failed to read {document} in {path:?}")]
    ReadMetadata {
        /// The mod file's path.
        path: PathBuf,
        /// The archive member being read.
        document: &'static str,
        /// The underlying archive or text decoding error.
        #[source]
        source: anyhow::Error,
    },
    /// A recognized document could not be parsed into identification metadata.
    #[error("Quilt: failed to parse {document} in {path:?}")]
    ParseMetadata {
        /// The mod file's path.
        path: PathBuf,
        /// The archive member being parsed.
        document: &'static str,
        /// The original parser or field error.
        #[source]
        source: anyhow::Error,
    },
    /// A declared nested archive could not be read or identified.
    #[error("Quilt: failed to identify nested JARs in {path:?}")]
    NestedIdentification {
        /// The outer mod file's path.
        path: PathBuf,
        /// The nested path and its original archive or loader error.
        #[source]
        source: anyhow::Error,
    },
}
