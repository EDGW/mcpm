//! File-only Mod backups over the content cache, independent of CLI and presentation.

use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    blocking,
    local::{
        cache::{CacheLinkPreference, TransferMethod, mods::ModsCacheDir},
        files::{self, Content, DirectoryProtection},
    },
};

mod delete;
pub use delete::delete;
mod manifest;
pub use manifest::{ChecksumMismatch, FileRecord, MANIFEST_NAME, Manifest};
mod tree;
mod verification;
pub use verification::VerificationError;
use verification::{check_file, check_inventory};

#[cfg(test)]
mod tests;

/// Cache-to-backup transfer policy; source-to-cache imports always copy content only.
#[derive(Debug, Clone, Copy)]
pub struct CreateOptions {
    pub mode: CacheLinkPreference,
    pub parallelism: NonZeroUsize,
}

#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions {
    pub mode: CacheLinkPreference,
    pub parallelism: NonZeroUsize,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Create,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
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

/// A successful publication; transfer methods correspond to the sorted manifest records.
#[derive(Debug)]
pub struct Outcome {
    operation: Operation,
    destination: PathBuf,
    manifest: Manifest,
    manifest_size: u64,
    methods: Vec<TransferMethod>,
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
    pub fn files(&self) -> impl Iterator<Item = (&FileRecord, TransferMethod)> {
        self.manifest
            .files()
            .iter()
            .zip(self.methods.iter().copied())
    }
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

/// Checks the manifest and every data file without creating files or consulting a cache.
pub fn verify(
    backup: &Path,
    parallelism: NonZeroUsize,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<VerifiedBackup> {
    let directory = tree::root(backup)?;
    let (manifest, manifest_size) = Manifest::read_with_size(&directory.join(MANIFEST_NAME))?;
    verify_tree(&directory.join("data"), &manifest, parallelism, &mut emit)?;
    Ok(VerifiedBackup {
        directory,
        manifest,
        manifest_size,
    })
}

/// Creates a complete verified backup; successful content imports survive later failures.
pub fn create(
    source: &Path,
    destination: &Path,
    cache: &ModsCacheDir,
    options: CreateOptions,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    let source = tree::root(source)?;
    let destination = tree::Destination::inspect(destination, false)?;
    tree::disjoint(&source, &destination.path)?;
    tree::disjoint(&source, cache.path())?;
    tree::disjoint(&destination.path, cache.path())?;
    let inventory = tree::scan(&source)?;
    tracing::info!(
        files = inventory.files.len(),
        "Preparing backup cache contents"
    );
    let records = run_files(
        inventory.files.len(),
        options.parallelism,
        Stage::Cache,
        |index| {
            let relative = &inventory.files[index];
            let path = tree::file_path(&source, relative)?;
            let original = Content::open(&path)?.context("backup source disappeared")?;
            let key = cache
                .import(&path, CacheLinkPreference::Copy)
                .with_context(|| format!("failed to cache backup file {path:?}"))?;
            original.validate()?;
            Ok(FileRecord::new(relative.clone(), key, original.size()))
        },
        &mut emit,
    )?;
    let manifest = Manifest::new(records)?;
    // The source set and every file must still match before backup generation starts.
    verify_tree(&source, &manifest, options.parallelism, &mut emit)?;
    let workspace = tempfile::tempdir_in(destination.path.parent().unwrap())?;
    let staged = workspace.path().join("new");
    let data = staged.join("data");
    fs::create_dir_all(&data)?;
    let result = (|| {
        let methods = run_files(
            manifest.files().len(),
            options.parallelism,
            Stage::Transfer,
            |index| {
                let record = &manifest.files()[index];
                let path = data.join(record.path());
                fs::create_dir_all(path.parent().unwrap())?;
                let method = cache
                    .export_file(&record.blake3(), &path, options.mode)
                    .with_context(|| format!("failed to create backup file {:?}", record.path()))?;
                check_file(&path, record)?;
                files::protect_file_contents(&path)
                    .with_context(|| format!("failed to protect backup file {path:?}"))?;
                Ok(method)
            },
            &mut emit,
        )?;
        check_inventory(&data, &manifest)?;
        // Guards restore directory access before workspace cleanup on every error path.
        let mut protections = Vec::new();
        for path in tree::scan(&data)?.directories.into_iter().rev() {
            protections.push(
                DirectoryProtection::protect(&path)
                    .with_context(|| format!("failed to protect backup directory {path:?}"))?,
            );
        }
        let manifest_path = staged.join(MANIFEST_NAME);
        let manifest_size = manifest.write(&manifest_path)?;
        files::protect_file_contents(&manifest_path)
            .context("failed to protect backup manifest contents")?;
        destination.commit(&staged, &workspace.path().join("previous"))?;
        for protection in protections {
            protection.persist();
        }
        Ok((methods, manifest_size))
    })();
    let (methods, manifest_size) = finish_workspace(workspace, result)?;
    Ok(Outcome {
        operation: Operation::Create,
        destination: destination.path,
        manifest,
        manifest_size,
        methods,
    })
}

/// Applies a self-contained backup without opening or populating a cache.
pub fn apply(
    backup: &Path,
    destination: &Path,
    options: ApplyOptions,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    let backup = tree::root(backup)?;
    let destination = tree::Destination::inspect(destination, options.force)?;
    tree::disjoint(&backup, &destination.path)?;
    let VerifiedBackup {
        manifest,
        manifest_size,
        ..
    } = verify(&backup, options.parallelism, &mut emit)?;
    tree::validate_target_names(manifest.files())?;
    let data = backup.join("data");
    let workspace = tempfile::tempdir_in(destination.path.parent().unwrap())?;
    let staged = workspace.path().join("new");
    fs::create_dir(&staged)?;
    let result = (|| {
        let methods = run_files(
            manifest.files().len(),
            options.parallelism,
            Stage::Transfer,
            |index| {
                let record = &manifest.files()[index];
                let source = tree::file_path(&data, record.path())?;
                let mut content = Content::open(&source)?.context("backup file disappeared")?;
                let path = staged.join(record.path());
                fs::create_dir_all(path.parent().unwrap())?;
                let method = files::stage(&mut content, &path, options.mode)
                    .with_context(|| format!("failed to apply backup file {:?}", record.path()))?;
                check_file(&path, record)?;
                content.validate()?;
                if method == TransferMethod::HardLink {
                    files::protect_file_contents(&path)?;
                }
                Ok(method)
            },
            &mut emit,
        )?;
        check_inventory(&staged, &manifest)?;
        check_inventory(&data, &manifest)?;
        destination.commit(&staged, &workspace.path().join("previous"))?;
        Ok(methods)
    })();
    let methods = finish_workspace(workspace, result)?;
    Ok(Outcome {
        operation: Operation::Apply,
        destination: destination.path,
        manifest,
        manifest_size,
        methods,
    })
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
