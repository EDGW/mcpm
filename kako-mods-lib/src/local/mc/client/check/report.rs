use std::path::PathBuf;

use thiserror::Error;

use super::super::{assets::AssetError, libraries::LibraryError};
use super::{CheckOptionValue, CheckOptions};

/// A report covers only the requested categories, not game launchability.
#[derive(Debug)]
pub struct CheckReport {
    pub version: String,
    pub options: CheckOptions,
    pub files: Vec<CheckedFile>,
    pub issues: Vec<CheckIssue>,
}

impl CheckReport {
    /// True only when a nonempty selection has no issues and every selected file
    /// was verified by the requested policy. Missing required verification metadata remains unverified.
    pub fn is_intact(&self) -> bool {
        !self.options.is_empty()
            && self.issues.is_empty()
            && self
                .files
                .iter()
                .all(|file| file.status == FileCheckStatus::Verified)
    }
    pub fn has_errors(&self) -> bool {
        !self.issues.is_empty()
    }
    pub fn unverified_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.status == FileCheckStatus::Unverified)
            .count()
    }
}

#[derive(Debug)]
pub struct CheckedFile {
    pub target: CheckTarget,
    pub path: PathBuf,
    pub status: FileCheckStatus,
    pub mode: CheckOptionValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCheckStatus {
    Verified,
    Unverified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTarget {
    Version,
    VersionJar,
    Library {
        name: String,
        classifier: Option<String>,
    },
    AssetIndex {
        id: String,
    },
    AssetObject {
        name: String,
    },
}

#[derive(Debug)]
pub struct CheckIssue {
    pub target: CheckTarget,
    pub path: Option<PathBuf>,
    pub problem: CheckProblem,
}

#[derive(Debug, Error)]
pub enum CheckProblem {
    #[error("version metadata could not be loaded")]
    Version {
        #[source]
        source: anyhow::Error,
    },
    #[error("version belongs to Minecraft root {actual:?}, not {expected:?}")]
    RootMismatch { expected: PathBuf, actual: PathBuf },
    #[error("failed to access local resource")]
    Io {
        #[source]
        source: std::io::Error,
    },
    #[error("file is missing")]
    MissingFile,
    #[error("resource is not a regular file")]
    NotAFile,
    #[error("size mismatch: expected {expected} bytes, found {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("SHA-1 mismatch: expected one of {expected:?}, found {actual}")]
    HashMismatch {
        expected: Vec<String>,
        actual: String,
    },
    #[error("invalid SHA-1 declaration {value:?}")]
    InvalidChecksum { value: String },
    #[error("invalid library metadata")]
    Library {
        #[source]
        source: LibraryError,
    },
    #[error("invalid asset metadata or index")]
    Assets {
        #[source]
        source: AssetError,
    },
    #[error("missing manifest metadata: {field}")]
    MissingMetadata { field: &'static str },
}
