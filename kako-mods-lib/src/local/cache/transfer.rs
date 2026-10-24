//! Explicit import/export policy, verified publication and actual transfer outcomes.

use super::{
    CacheDir,
    hash::Blake3Hash,
    index, layout,
    storage::{self, Content},
};
use crate::local::files;
use crate::local::files::stage;
pub use crate::local::files::{CacheLinkPreference, TransferMethod};
use anyhow::{Context, Result, ensure};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

impl CacheDir {
    /// Imports content and returns its key; use `import_with_outcome` for publication details.
    pub fn import(&self, file: &Path, preference: CacheLinkPreference) -> Result<Blake3Hash> {
        Ok(self.import_with_outcome(file, preference)?.key)
    }

    /// Exports content; use `export_with_outcomes` to observe the method per target.
    pub fn export(
        &self,
        key: &Blake3Hash,
        dest: &[PathBuf],
        preference: CacheLinkPreference,
    ) -> Result<Option<()>> {
        Ok(self
            .export_with_outcomes(key, dest, preference)?
            .map(|_| ()))
    }

    /// Exports one object and returns its actual transfer method. Missing content
    /// is an error; use batch `export` when absence should be represented as `None`.
    pub fn export_file(
        &self,
        key: &Blake3Hash,
        destination: &Path,
        preference: CacheLinkPreference,
    ) -> Result<TransferMethod> {
        let outcomes = self
            .export_with_outcomes(key, &[destination.to_owned()], preference)?
            .with_context(|| format!("cache object {key} does not exist"))?;
        Ok(outcomes
            .into_iter()
            .next()
            .expect("one export destination")
            .method)
    }

    /// Imports regular-file content, reusing a verified existing object.
    ///
    /// New objects are staged, verified and protected before publication without
    /// overwriting an existing object. Hard-link protection also affects the source.
    pub fn import_with_outcome(
        &self,
        file: &Path,
        preference: CacheLinkPreference,
    ) -> Result<ImportOutcome> {
        let mut source = Content::open(file)?
            .with_context(|| format!("import source does not exist: {file:?}"))?;
        let quick = source.quick_hash()?;
        let key = source.hash()?;
        if self.open_verified(&key)?.is_some() {
            source.validate()?;
            index::record(&self.path, &quick, &key);
            return Ok(ImportOutcome {
                key,
                published: false,
                method: None,
            });
        }
        let destination = self.data_path(&key);
        let parent = destination.parent().expect("cache object has a shard");
        layout::create_shard(&self.path, &key)?;
        let staging = tempfile::tempdir_in(parent)
            .with_context(|| format!("failed to stage cache import in {parent:?}"))?;
        let staged = staging.path().join("content");
        let method = stage(&mut source, &staged, preference)?;
        let mut content = Content::open(&staged)?.context("staged import is missing")?;
        ensure!(
            content.hash()?.as_bytes() == key.as_bytes(),
            "import source changed: {file:?}"
        );
        files::protect_file_contents(&staged)
            .with_context(|| format!("failed to protect cached content {staged:?}"))?;
        content.validate()?;
        source.validate()?;
        let published = match storage::publish(&staged, &destination) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.open_verified(&key)?
                    .context("concurrently published cache object disappeared")?;
                false
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to publish cached content {destination:?}"));
            }
        };
        source.validate()?;
        index::record(&self.path, &quick, &key);
        Ok(ImportOutcome {
            key,
            published,
            method: published.then_some(method),
        })
    }

    /// Exports verified content to absent files with existing parents outside the cache.
    ///
    /// Returns `None` when content is missing. Each destination is published without
    /// overwriting; a later failure leaves earlier completed exports in place.
    /// Copies are writable, while hard links share the protected cache contents.
    pub fn export_with_outcomes(
        &self,
        key: &Blake3Hash,
        dest: &[PathBuf],
        preference: CacheLinkPreference,
    ) -> Result<Option<Vec<ExportOutcome>>> {
        let Some(mut content) = self.open_verified(key)? else {
            return Ok(None);
        };
        let mut destinations = Vec::with_capacity(dest.len());
        for path in dest {
            let filename = path
                .file_name()
                .with_context(|| format!("invalid export path {path:?}"))?;
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent = fs::canonicalize(parent)
                .with_context(|| format!("failed to resolve export parent for {path:?}"))?;
            ensure!(
                !parent.starts_with(&self.path),
                "export destination is inside the cache: {path:?}"
            );
            let destination = parent.join(filename);
            ensure!(
                !destinations.contains(&destination),
                "duplicate export destination {path:?}"
            );
            match fs::symlink_metadata(&destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect export destination {destination:?}")
                    });
                }
                Ok(_) => anyhow::bail!("export destination already exists: {destination:?}"),
            }
            destinations.push(destination);
        }
        let mut outcomes = Vec::with_capacity(destinations.len());
        for destination in destinations {
            let parent = destination.parent().expect("resolved export has a parent");
            let staging = tempfile::tempdir_in(parent)
                .with_context(|| format!("failed to stage export in {parent:?}"))?;
            let staged = staging.path().join("content");
            let method = stage(&mut content, &staged, preference)?;
            let mut copied = Content::open(&staged)?.context("staged export is missing")?;
            ensure!(
                copied.hash()?.as_bytes() == key.as_bytes(),
                "cached content changed during export"
            );
            copied.validate()?;
            content.validate()?;
            storage::publish(&staged, &destination)
                .with_context(|| format!("failed to publish exported content {destination:?}"))?;
            content.validate()?;
            outcomes.push(ExportOutcome {
                destination,
                method,
            });
        }
        Ok(Some(outcomes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOutcome {
    pub key: Blake3Hash,
    /// True only if this call published the canonical content name.
    pub published: bool,
    /// None when another publication is reused; no source relinking is implied.
    pub method: Option<TransferMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    pub destination: PathBuf,
    pub method: TransferMethod,
}
