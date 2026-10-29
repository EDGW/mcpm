//! Errors from resolving client launch metadata, configuration, libraries and arguments.

use std::path::PathBuf;

use thiserror::Error;

use crate::local::mc::client::{LibraryError, ResourcePathError};

use super::super::ArgumentError;

#[derive(Debug, Error)]
pub enum LaunchPlanError {
    #[error("version {version:?} belongs to a different Minecraft root")]
    RootMismatch { version: String },
    #[error("cannot load launch metadata")]
    Version {
        #[source]
        source: anyhow::Error,
    },
    #[error("launch metadata is missing {field}")]
    MissingMetadata { field: &'static str },
    #[error("invalid launch configuration: {field}: {reason}")]
    Configuration {
        field: &'static str,
        reason: &'static str,
    },
    #[error("cannot resolve launch library {library:?}")]
    Library {
        library: String,
        #[source]
        source: LibraryError,
    },
    #[error(transparent)]
    Argument(#[from] ArgumentError),
    #[error(transparent)]
    Path(#[from] ResourcePathError),
    #[error("{operation}: {path:?}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot load asset index for legacy resource mapping")]
    Assets(#[from] crate::local::mc::client::AssetError),
}
