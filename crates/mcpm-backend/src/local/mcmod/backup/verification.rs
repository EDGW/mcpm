//! Typed backup integrity failures and exact file-set/content verification.

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{FileRecord, Manifest, tree};
use crate::local::{
    cache::mods::ModsCacheDir,
    files::{Content, hash::Blake3Hash},
};

pub(super) fn check_inventory(root: &Path, manifest: &Manifest) -> Result<()> {
    let inventory = tree::scan(root)?;
    let actual: BTreeSet<_> = inventory.files.iter().map(String::as_str).collect();
    let expected: BTreeSet<_> = manifest.files().iter().map(FileRecord::path).collect();
    check_file_set(root, &expected, &actual, false)
}

/// A missing data root represents an empty set; malformed or unreadable roots remain errors.
pub(super) fn check_data_inventory(
    root: &Path,
    manifest: &Manifest,
    allow_missing: bool,
) -> Result<BTreeSet<String>> {
    let actual: BTreeSet<String> = match fs::symlink_metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect backup data {root:?}"));
        }
        Ok(_) => {
            let inventory = tree::scan(root)?;
            let directories: BTreeSet<_> = inventory.directories.into_iter().collect();
            for record in manifest.files() {
                ensure!(
                    !directories.contains(&root.join(record.path())),
                    "expected a regular backup file, found a directory: {:?}",
                    root.join(record.path())
                );
            }
            inventory.files.into_iter().collect()
        }
    };
    let names = actual.iter().map(String::as_str).collect();
    let expected = manifest.files().iter().map(FileRecord::path).collect();
    check_file_set(root, &expected, &names, allow_missing)?;
    Ok(actual)
}

fn check_file_set(
    root: &Path,
    expected: &BTreeSet<&str>,
    actual: &BTreeSet<&str>,
    allow_missing: bool,
) -> Result<()> {
    if !actual.is_subset(expected) || (!allow_missing && actual != expected) {
        return Err(VerificationError::FileSet {
            directory: root.to_owned(),
            missing: expected
                .difference(actual)
                .map(|path| (*path).to_owned())
                .collect(),
            unexpected: actual
                .difference(expected)
                .map(|path| (*path).to_owned())
                .collect(),
        }
        .into());
    }
    Ok(())
}

/// Cache owns content hashing and layout; backup checks the manifest's expected size.
pub(super) fn check_cached_file(cache: &ModsCacheDir, record: &FileRecord) -> Result<()> {
    let actual = cache.verified_size(&record.blake3())?.with_context(|| {
        format!(
            "cache object {} required for {:?} does not exist",
            record.blake3(),
            record.path()
        )
    })?;
    if actual != record.size() {
        return Err(VerificationError::FileSize {
            path: cache.content_path(&record.blake3()),
            expected: record.size(),
            actual,
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
