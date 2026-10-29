//! File-only Mod backups over the content cache, independent of CLI and presentation.

use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    blocking,
    local::cache::{TransferMethod, mods::ModsCacheDir},
};

mod apply;
pub use apply::{ApplyOptions, apply, apply_with_cache};
mod create;
pub use create::{CreateOptions, CreateStorage, create, create_with_cache};
mod checksum;
pub use checksum::{Comparison, DirectoryChecksum, FileDifference, checksum};
mod delete;
pub use delete::delete;
mod manifest;
pub use manifest::{ChecksumMismatch, FileRecord, MANIFEST_NAME, Manifest};
mod sources;
mod tree;
mod verification;
pub use verification::VerificationError;
use verification::{check_file, check_inventory};

#[cfg(test)]
mod tests;

// region: verify

/// Checks the manifest and every data file without creating files or consulting a cache.
pub fn verify(
    backup: &Path,
    parallelism: NonZeroUsize,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<VerifiedBackup> {
    verify_inner(
        backup,
        None::<fn() -> Result<ModsCacheDir>>,
        parallelism,
        emit,
    )
}

/// Fully verifies restoration sources, consulting the supplied cache only for missing backup files.
/// Does not create files, initialize caches or populate backup data.
pub fn verify_with_cache(
    backup: &Path,
    open_cache: impl FnOnce() -> Result<ModsCacheDir>,
    parallelism: NonZeroUsize,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<VerifiedBackup> {
    verify_inner(backup, Some(open_cache), parallelism, emit)
}

fn verify_inner(
    backup: &Path,
    open_cache: Option<impl FnOnce() -> Result<ModsCacheDir>>,
    parallelism: NonZeroUsize,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<VerifiedBackup> {
    let plan = sources::Sources::inspect(backup, open_cache).map_err(|error| {
        if matches!(error.downcast_ref::<VerificationError>(), Some(VerificationError::FileSet { missing, unexpected, .. }) if !missing.is_empty() && unexpected.is_empty()) {
            error.context("backup is not self-contained; cache-assisted restoration requires a verified cache")
        } else { error }
    })?;
    plan.verify(parallelism, &mut emit)?;
    Ok(plan.backup)
}

/// A complete read-only integrity check, with no transfer or cache ownership assumptions.
#[derive(Debug)]
pub struct VerifiedBackup {
    directory: PathBuf,
    manifest: Manifest,
    manifest_size: u64,
}
impl VerifiedBackup {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn manifest_size(&self) -> u64 {
        self.manifest_size
    }
}

// endregion

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Create,
    CreateCacheOnly,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Checksum,
    Cache,
    Verify,
    Transfer,
}

/// Observations contain no formatted labels or terminal policy.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    Progress {
        stage: Stage,
        completed: usize,
        total: usize,
    },
    StageDone(Stage),
}

/// A successful publication; per-file dispositions correspond to the sorted manifest records.
#[derive(Debug)]
pub struct Outcome {
    operation: Operation,
    destination: PathBuf,
    manifest: Manifest,
    manifest_size: u64,
    dispositions: Vec<FileDisposition>,
}
impl Outcome {
    pub fn operation(&self) -> Operation {
        self.operation
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    /// Exact encoded manifest bytes, independent of semantic checksum encoding.
    pub fn manifest_size(&self) -> u64 {
        self.manifest_size
    }
    pub fn files(&self) -> impl Iterator<Item = (&FileRecord, FileDisposition)> {
        self.manifest
            .files()
            .iter()
            .zip(self.dispositions.iter().copied())
    }
}

/// A file was retained only in cache, or transferred from one verified source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDisposition {
    CacheOnly,
    Transferred {
        source: FileSource,
        method: TransferMethod,
    },
}

/// The content resource used to generate an exported file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSource {
    Backup,
    Cache,
}

fn verify_tree(
    root: &Path,
    manifest: &Manifest,
    parallelism: NonZeroUsize,
    emit: &mut impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    check_inventory(root, manifest)?;
    run_files(
        manifest.files().len(),
        parallelism,
        Stage::Verify,
        |index| {
            let record = &manifest.files()[index];
            check_file(&tree::file_path(root, record.path())?, record)
        },
        emit,
    )?;
    check_inventory(root, manifest)
}

fn run_files<T: Send>(
    count: usize,
    parallelism: NonZeroUsize,
    stage: Stage,
    work: impl Fn(usize) -> Result<T> + Sync,
    emit: &mut impl FnMut(Event) -> Result<()>,
) -> Result<Vec<T>> {
    let mut results: Vec<Option<T>> = (0..count).map(|_| None).collect();
    let mut completed = 0;
    emit(Event::Progress {
        stage,
        completed,
        total: count,
    })?;
    blocking::run(
        count,
        parallelism.get(),
        work,
        || Ok(()),
        |index, result| {
            results[index] = Some(result?);
            completed += 1;
            emit(Event::Progress {
                stage,
                completed,
                total: count,
            })
        },
    )?;
    emit(Event::StageDone(stage))?;
    Ok(results
        .into_iter()
        .map(|result| result.expect("worker completed"))
        .collect())
}

/// A failed rollback or cleanup must never delete the only retained previous tree.
fn finish_workspace<T>(workspace: tempfile::TempDir, result: Result<T>) -> Result<T> {
    let previous = workspace.path().join("previous");
    if previous.exists() {
        match result {
            Err(error) => {
                let retained = workspace.keep();
                return Err(error)
                    .with_context(|| format!("recovery files retained in {retained:?}"));
            }
            Ok(value) => {
                if let Err(error) = fs::remove_dir_all(&previous) {
                    let retained = workspace.keep();
                    return Err(error).with_context(|| {
                        format!(
                            "new directory was applied; previous directory retained in {retained:?}"
                        )
                    });
                }
                return Ok(value);
            }
        }
    }
    result
}
