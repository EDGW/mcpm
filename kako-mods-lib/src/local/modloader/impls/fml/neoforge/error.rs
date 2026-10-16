use std::path::PathBuf;

use thiserror::Error;

/// Errors reported by the NeoForge identification boundary.
#[derive(Debug, Error)]
pub enum NeoForgeError {
    #[error("NeoForge: failed to identify server launch")]
    ServerIdentification {
        #[source]
        source: anyhow::Error,
    },

    /// Recognized client library declarations are invalid.
    #[error("NeoForge: failed to identify client loader version")]
    ClientIdentification {
        #[source]
        source: crate::local::modloader::impls::ClientLibraryError,
    },

    /// The archive could not be opened or decoded as ZIP.
    #[error("NeoForge: failed to open mod archive {path:?}")]
    OpenArchive {
        /// The mod file's path.
        path: PathBuf,
        /// The underlying filesystem or archive error.
        #[source]
        source: anyhow::Error,
    },
    /// A recognized metadata document could not be read.
    #[error("NeoForge: failed to read {document} in {path:?}")]
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
    #[error("NeoForge: failed to parse {document} in {path:?}")]
    ParseMetadata {
        /// The mod file's path.
        path: PathBuf,
        /// The archive member being parsed.
        document: &'static str,
        /// The original parser or field error.
        #[source]
        source: anyhow::Error,
    },
    /// A version reference is missing, malformed, cyclic, or cannot be read.
    #[error("NeoForge: failed to resolve version {expression:?} for mod {modid} in {path:?}")]
    ResolveVersion {
        /// The mod file's path.
        path: PathBuf,
        /// The document declaring the version.
        document: &'static str,
        /// The mod whose version could not be resolved.
        modid: String,
        /// The original version expression.
        expression: String,
        /// The underlying reference or manifest error.
        #[source]
        source: anyhow::Error,
    },
    /// A declared nested archive could not be read or identified.
    #[error("NeoForge: failed to identify nested JARs in {path:?}")]
    NestedIdentification {
        /// The outer mod file's path.
        path: PathBuf,
        /// The nested path and its original archive or loader error.
        #[source]
        source: anyhow::Error,
    },
}
