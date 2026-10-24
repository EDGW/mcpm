//! Typed backup integrity failures and exact file-set/content verification.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::Result;

use super::{FileRecord, Manifest, tree};
use crate::local::files::{Content, hash::Blake3Hash};

/// Structured differences for callers to present without parsing error messages.
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error(
        "backup file inventory differs in {directory:?}; missing: {missing:?}; unexpected: {unexpected:?}"
    )]
    FileSet {
        directory: PathBuf,
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
    #[error("file required by the backup manifest is missing: {path:?}")]
    MissingFile { path: PathBuf },
    #[error("backup file size mismatch: {path:?}; expected {expected} bytes, found {actual} bytes")]
    FileSize {
        path: PathBuf,
        expected: u64,
        actual: u64,
    },
    #[error("backup file BLAKE3 mismatch: {path:?}; expected {expected}, found {actual}")]
    FileHash {
        path: PathBuf,
        expected: Blake3Hash,
        actual: Blake3Hash,
    },
}

pub(super) fn check_inventory(root: &Path, manifest: &Manifest) -> Result<()> {
    let inventory = tree::scan(root)?;
    let actual: BTreeSet<_> = inventory.files.iter().map(String::as_str).collect();
    let expected: BTreeSet<_> = manifest.files().iter().map(FileRecord::path).collect();
    if actual != expected {
        return Err(VerificationError::FileSet {
            directory: root.to_owned(),
            missing: expected
                .difference(&actual)
                .map(|path| (*path).to_owned())
                .collect(),
            unexpected: actual
                .difference(&expected)
                .map(|path| (*path).to_owned())
                .collect(),
        }
        .into());
    }
    Ok(())
}

pub(super) fn check_file(path: &Path, record: &FileRecord) -> Result<()> {
    let mut content = Content::open(path)?.ok_or_else(|| VerificationError::MissingFile {
        path: path.to_owned(),
    })?;
    if content.size() != record.size() {
        return Err(VerificationError::FileSize {
            path: path.to_owned(),
            expected: record.size(),
            actual: content.size(),
        }
        .into());
    }
    let actual = content.hash()?;
    if actual != record.blake3() {
        return Err(VerificationError::FileHash {
            path: path.to_owned(),
            expected: record.blake3(),
            actual,
        }
        .into());
    }
    Ok(())
}
