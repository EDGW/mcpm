//! Shared source selection and complete validation for read-only verification and staged application.

use std::{collections::BTreeSet, num::NonZeroUsize, path::Path};

use anyhow::{Context, Result, ensure};

use super::{
    Event, FileSource, MANIFEST_NAME, Manifest, Stage, VerifiedBackup, run_files, tree,
    verification::{check_cached_file, check_data_inventory, check_file},
};
use crate::local::cache::mods::ModsCacheDir;

/// A single source plan; cache access is lazy and only missing backup files use it.
pub(super) struct Sources {
    pub backup: VerifiedBackup,
    pub files: Vec<FileSource>,
    pub cache: Option<ModsCacheDir>,
    present: BTreeSet<String>,
    allow_missing: bool,
}

impl Sources {
    pub fn inspect(
        backup: &Path,
        open_cache: Option<impl FnOnce() -> Result<ModsCacheDir>>,
    ) -> Result<Self> {
        let directory = tree::root(backup)?;
        let (manifest, manifest_size) = Manifest::read_with_size(&directory.join(MANIFEST_NAME))?;
        let allow_missing = open_cache.is_some();
        let present = check_data_inventory(&directory.join("data"), &manifest, allow_missing)?;
        let files: Vec<_> = manifest
            .files()
            .iter()
            .map(|record| {
                if present.contains(record.path()) {
                    FileSource::Backup
                } else {
                    FileSource::Cache
                }
            })
            .collect();
        let cache = if files.contains(&FileSource::Cache) {
            let cache = open_cache.context("backup is not self-contained")?()
                .context("failed to open fallback cache for missing backup files")?;
            tree::disjoint(&directory, cache.path())?;
            Some(cache)
        } else {
            None
        };
        Ok(Self {
            backup: VerifiedBackup {
                directory,
                manifest,
                manifest_size,
            },
            files,
            cache,
            present,
            allow_missing,
        })
    }

    /// Filesystem-specific destination checks belong to application, not read-only verification.
    pub fn check_destination(&self, destination: &Path) -> Result<()> {
        tree::disjoint(self.backup.directory(), destination)?;
        if let Some(cache) = &self.cache {
            tree::disjoint(destination, cache.path())?;
        }
        tree::validate_target_names(self.backup.manifest().files())
    }

    pub fn verify(
        &self,
        parallelism: NonZeroUsize,
        emit: &mut impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        let data = self.backup.directory().join("data");
        run_files(
            self.files.len(),
            parallelism,
            Stage::Verify,
            |index| {
                let record = &self.backup.manifest().files()[index];
                match self.files[index] {
                    FileSource::Backup => {
                        check_file(&tree::file_path(&data, record.path())?, record)
                    }
                    FileSource::Cache => check_cached_file(
                        self.cache
                            .as_ref()
                            .context("fallback cache is unavailable")?,
                        record,
                    ),
                }
                .with_context(|| format!("failed to verify content for {:?}", record.path()))
            },
            emit,
        )?;
        self.recheck_inventory()
    }

    pub fn recheck_inventory(&self) -> Result<()> {
        ensure!(
            check_data_inventory(
                &self.backup.directory().join("data"),
                self.backup.manifest(),
                self.allow_missing
            )? == self.present,
            "backup file inventory changed during operation"
        );
        Ok(())
    }
}
