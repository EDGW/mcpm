//! Read-only directory checksums and comparisons using the sole manifest protocol.

use std::{
    cmp::Ordering,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::{
    Event, FileRecord, MANIFEST_NAME, Manifest, Stage, run_files, tree,
    verification::check_inventory,
};
use crate::local::files::{Content, hash::Blake3Hash};

/// Hashes every regular file using portable relative paths, without consulting a cache.
///
/// Uses the current manifest version. Empty directories are omitted. Files are
/// checked for changes while being read, and the file inventory is checked again
/// afterwards; this does not provide an atomic snapshot of a changing directory.
pub fn checksum(
    directory: &Path,
    parallelism: NonZeroUsize,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<DirectoryChecksum> {
    let directory = tree::root(directory)?;
    let inventory = tree::scan(&directory)?;
    let records = run_files(
        inventory.files.len(),
        parallelism,
        Stage::Checksum,
        |index| {
            let relative = &inventory.files[index];
            let path = tree::file_path(&directory, relative)?;
            let mut content = Content::open(&path)?
                .with_context(|| format!("directory file disappeared: {path:?}"))?;
            Ok(FileRecord::new(
                relative.clone(),
                content.hash()?,
                content.size(),
            ))
        },
        &mut |event| match event {
            Event::StageDone(_) => Ok(()),
            event => emit(event),
        },
    )?;
    let manifest = Manifest::new(records)?;
    check_inventory(&directory, &manifest)
        .context("directory file inventory changed during checksum calculation")?;
    emit(Event::StageDone(Stage::Checksum))?;
    Ok(DirectoryChecksum {
        directory,
        manifest,
    })
}

/// A directory's computed file records; its manifest is the only checksum owner.
#[derive(Debug)]
pub struct DirectoryChecksum {
    directory: PathBuf,
    manifest: Manifest,
}

impl DirectoryChecksum {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Reads and validates only the backup's manifest, without inspecting `data/`.
    pub fn compare_with_backup(&self, backup: &Path) -> Result<Comparison> {
        let backup = tree::root(backup)?;
        let expected = Manifest::read(&backup.join(MANIFEST_NAME))?;
        Ok(self.compare_manifest(&expected))
    }

    /// Compares already validated records without rereading the directory.
    pub fn compare_manifest(&self, expected: &Manifest) -> Comparison {
        let mut comparison = self.compare_checksum(expected.checksum());
        if comparison.matches() {
            return comparison;
        }
        let mut expected = expected.files().iter().peekable();
        let mut actual = self.manifest.files().iter().peekable();
        loop {
            match (expected.peek(), actual.peek()) {
                (Some(left), Some(right)) => match left.path().cmp(right.path()) {
                    Ordering::Less => comparison.differences.push(FileDifference::Missing {
                        expected: expected.next().unwrap().clone(),
                    }),
                    Ordering::Greater => comparison.differences.push(FileDifference::Unexpected {
                        actual: actual.next().unwrap().clone(),
                    }),
                    Ordering::Equal => {
                        let left = expected.next().unwrap();
                        let right = actual.next().unwrap();
                        if left.size() != right.size() || left.blake3() != right.blake3() {
                            comparison.differences.push(FileDifference::Changed {
                                expected: left.clone(),
                                actual: right.clone(),
                            });
                        }
                    }
                },
                (Some(_), None) => comparison.differences.extend(
                    expected
                        .by_ref()
                        .cloned()
                        .map(|expected| FileDifference::Missing { expected }),
                ),
                (None, Some(_)) => comparison.differences.extend(
                    actual
                        .by_ref()
                        .cloned()
                        .map(|actual| FileDifference::Unexpected { actual }),
                ),
                (None, None) => break,
            }
        }
        comparison
    }

    /// Compares a v1 semantic checksum; a checksum alone cannot describe file differences.
    pub fn compare_checksum(&self, expected: Blake3Hash) -> Comparison {
        Comparison {
            expected,
            actual: self.manifest.checksum(),
            differences: Vec::new(),
        }
    }
}

/// A completed comparison. A mismatch is data, rather than an operational error.
#[derive(Debug)]
pub struct Comparison {
    expected: Blake3Hash,
    actual: Blake3Hash,
    differences: Vec<FileDifference>,
}

impl Comparison {
    pub fn matches(&self) -> bool {
        self.expected == self.actual
    }

    pub fn expected(&self) -> Blake3Hash {
        self.expected
    }

    pub fn actual(&self) -> Blake3Hash {
        self.actual
    }

    /// Empty for matching inputs and for comparisons against a bare checksum.
    pub fn differences(&self) -> &[FileDifference] {
        &self.differences
    }
}

/// File differences retain both records so callers can explain size and hash changes.
#[derive(Debug)]
pub enum FileDifference {
    Missing {
        expected: FileRecord,
    },
    Unexpected {
        actual: FileRecord,
    },
    Changed {
        expected: FileRecord,
        actual: FileRecord,
    },
}
