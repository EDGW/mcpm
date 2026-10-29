//! Shared staged application with strict backup sources or lazy, verified cache fallback.

use std::{fs, num::NonZeroUsize, path::Path};

use anyhow::{Context, Result};

use super::{
    Event, FileDisposition, FileSource, Operation, Outcome, Stage, finish_workspace, run_files,
    sources::Sources,
    tree,
    verification::{check_file, check_inventory},
};
use crate::local::{
    cache::{CacheLinkPreference, TransferMethod, mods::ModsCacheDir},
    files::{self, Content},
};

/// Applies a strictly self-contained backup without opening or populating a cache.
pub fn apply(
    backup: &Path,
    destination: &Path,
    options: ApplyOptions,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    apply_inner(
        backup,
        destination,
        None::<fn() -> Result<ModsCacheDir>>,
        options,
        emit,
    )
}

/// Uses a verified cache only for missing backup files. The opener is not called for complete backups.
pub fn apply_with_cache(
    backup: &Path,
    destination: &Path,
    open_cache: impl FnOnce() -> Result<ModsCacheDir>,
    options: ApplyOptions,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    apply_inner(backup, destination, Some(open_cache), options, emit)
}

/// Source-to-target transfer policy and destination replacement behavior.
#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions {
    pub mode: CacheLinkPreference,
    pub parallelism: NonZeroUsize,
    pub force: bool,
}

fn apply_inner(
    backup: &Path,
    destination: &Path,
    open_cache: Option<impl FnOnce() -> Result<ModsCacheDir>>,
    options: ApplyOptions,
    mut emit: impl FnMut(Event) -> Result<()>,
) -> Result<Outcome> {
    let plan = Sources::inspect(backup, open_cache)?;
    let destination = tree::Destination::inspect(destination, options.force)?;
    plan.check_destination(&destination.path)?;
    plan.verify(options.parallelism, &mut emit)?;
    let manifest = plan.backup.manifest();
    let data = plan.backup.directory().join("data");
    let workspace = tempfile::tempdir_in(destination.path.parent().unwrap())?;
    let staged = workspace.path().join("new");
    fs::create_dir(&staged)?;
    let result = (|| {
        let dispositions = run_files(
            manifest.files().len(),
            options.parallelism,
            Stage::Transfer,
            |index| {
                let record = &manifest.files()[index];
                let path = staged.join(record.path());
                fs::create_dir_all(path.parent().unwrap())?;
                let source = plan.files[index];
                let method = match source {
                    FileSource::Backup => {
                        let source = tree::file_path(&data, record.path())?;
                        let mut content =
                            Content::open(&source)?.context("backup file disappeared")?;
                        let method =
                            files::stage(&mut content, &path, options.mode).with_context(|| {
                                format!("failed to apply backup file {:?}", record.path())
                            })?;
                        content.validate()?;
                        method
                    }
                    FileSource::Cache => plan
                        .cache
                        .as_ref()
                        .context("fallback cache is unavailable")?
                        .export_file(&record.blake3(), &path, options.mode)
                        .with_context(|| {
                            format!(
                                "failed to restore missing backup file {:?} from cache",
                                record.path()
                            )
                        })?,
                };
                check_file(&path, record)?;
                if method == TransferMethod::HardLink {
                    files::protect_file_contents(&path)?;
                }
                Ok(FileDisposition::Transferred { source, method })
            },
            &mut emit,
        )?;
        check_inventory(&staged, manifest)?;
        plan.recheck_inventory()?;
        destination.commit(&staged, &workspace.path().join("previous"))?;
        Ok(dispositions)
    })();
    let dispositions = finish_workspace(workspace, result)?;
    Ok(Outcome {
        operation: Operation::Apply,
        destination: destination.path,
        manifest: plan.backup.manifest,
        manifest_size: plan.backup.manifest_size,
        dispositions,
    })
}
