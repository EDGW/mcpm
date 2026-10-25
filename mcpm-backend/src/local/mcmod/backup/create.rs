//! Snapshot creation with shared cache import, full export or manifest-only publication.

use std::{fs, num::NonZeroUsize, path::Path};

use anyhow::{Context, Result};

use super::verification::{check_cached_file, check_file, check_inventory};
use super::{
    Event, FileDisposition, FileRecord, FileSource, MANIFEST_NAME, Manifest, Operation, Outcome,
    Stage, finish_workspace, run_files, tree, verify_tree,
};
use crate::local::{
    cache::{CacheLinkPreference, mods::ModsCacheDir},
    files::{self, Content, DirectoryProtection},
    runtime::RuntimeEnvironment,
};

/// Creates a verified full or manifest-only backup; successful content imports survive later failures.
pub fn create(
    source: &Path,
    destination: &Path,
    environment: &RuntimeEnvironment,
    options: CreateOptions,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    let cache = environment.cache.backup.open()?;
    create_with_cache(source, destination, cache, options, emit)
}

/// Creates a backup using an explicitly selected content resource, without choosing a runtime role.
pub fn create_with_cache(
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
    fs::create_dir(&staged)?;
    if matches!(options.storage, CreateStorage::Full(_)) {
        fs::create_dir(&data)?;
    }
    let result = (|| {
        let mut protections = Vec::new();
        let dispositions = match options.storage {
            CreateStorage::CacheOnly => run_files(
                manifest.files().len(),
                options.parallelism,
                Stage::Verify,
                |index| {
                    check_cached_file(cache, &manifest.files()[index])?;
                    Ok(FileDisposition::CacheOnly)
                },
                &mut emit,
            )?,
            CreateStorage::Full(mode) => {
                let dispositions = run_files(
                    manifest.files().len(),
                    options.parallelism,
                    Stage::Transfer,
                    |index| {
                        let record = &manifest.files()[index];
                        let path = data.join(record.path());
                        fs::create_dir_all(path.parent().unwrap())?;
                        let method = cache
                            .export_file(&record.blake3(), &path, mode)
                            .with_context(|| {
                                format!("failed to create backup file {:?}", record.path())
                            })?;
                        check_file(&path, record)?;
                        files::protect_file_contents(&path)
                            .with_context(|| format!("failed to protect backup file {path:?}"))?;
                        Ok(FileDisposition::Transferred {
                            source: FileSource::Cache,
                            method,
                        })
                    },
                    &mut emit,
                )?;
                check_inventory(&data, &manifest)?;
                // Restore directory access before staging cleanup on every failure path.
                for path in tree::scan(&data)?.directories.into_iter().rev() {
                    protections.push(
                        DirectoryProtection::protect(&path).with_context(|| {
                            format!("failed to protect backup directory {path:?}")
                        })?,
                    );
                }
                dispositions
            }
        };
        let manifest_path = staged.join(MANIFEST_NAME);
        let manifest_size = manifest.write(&manifest_path)?;
        files::protect_file_contents(&manifest_path)
            .context("failed to protect backup manifest contents")?;
        destination.commit(&staged, &workspace.path().join("previous"))?;
        for protection in protections {
            protection.persist();
        }
        Ok((dispositions, manifest_size))
    })();
    let (dispositions, manifest_size) = finish_workspace(workspace, result)?;
    Ok(Outcome {
        operation: match options.storage {
            CreateStorage::Full(_) => Operation::Create,
            CreateStorage::CacheOnly => Operation::CreateCacheOnly,
        },
        destination: destination.path,
        manifest,
        manifest_size,
        dispositions,
    })
}

/// Snapshot storage policy; source-to-cache imports always copy content only.
#[derive(Debug, Clone, Copy)]
pub struct CreateOptions {
    pub storage: CreateStorage,
    pub parallelism: NonZeroUsize,
}

/// Full backups export protected data; cache-only backups publish just the manifest.
#[derive(Debug, Clone, Copy)]
pub enum CreateStorage {
    Full(CacheLinkPreference),
    CacheOnly,
}
