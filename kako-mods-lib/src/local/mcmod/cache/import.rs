//! Deduplicated imports with no-clobber publication and explicit hard-link ownership.

use std::{fs, io, path::Path};

use super::{
    CacheError, CacheId, ImportMethod, ImportMode, ImportOutcome, content::ContentFile,
    error::IoContext, index, operations::Store, storage,
};
use crate::local::files;

impl Store {
    /// Publishes data only. Duplicate content never replaces the existing inode.
    pub(super) fn import(
        &self,
        source: &Path,
        mode: ImportMode,
    ) -> Result<ImportOutcome, CacheError> {
        let mut original = ContentFile::open(source)?;
        let id = original.id()?;
        let quick = original.quick_hash()?;
        let method = if self.reuse_data(id)? {
            None
        } else {
            self.import_new(source, &original, id, mode)?
        };
        original.validate()?;
        if let Err(error) = index::insert(&self.root, &quick, id) {
            tracing::debug!(%id, %error, "Skipping optional quick-index update");
        }
        Ok(ImportOutcome {
            id,
            published: method.is_some(),
            method,
        })
    }

    /// Transfer strategy applies only after a content lookup misses.
    fn import_new(
        &self,
        source: &Path,
        original: &ContentFile,
        id: CacheId,
        mode: ImportMode,
    ) -> Result<Option<ImportMethod>, CacheError> {
        let staging =
            tempfile::tempdir_in(&self.root).at("create import staging directory", &self.root)?;
        let staged = staging.path().join("data");
        let method = if mode == ImportMode::Copy {
            ImportMethod::Copy
        } else {
            match files::hard_link(source, &staged) {
                Ok(()) => {
                    let linked = ContentFile::open(&staged)?;
                    original.validate()?;
                    if linked.identity != original.identity {
                        return Err(CacheError::SourceChanged(source.to_owned()));
                    }
                    ImportMethod::HardLink
                }
                Err(error)
                    if mode == ImportMode::PreferHardLink && error.allows_copy_fallback() =>
                {
                    tracing::debug!(path = %source.display(), %error, "Hard-link import unsupported; copying instead");
                    ImportMethod::Copy
                }
                Err(error) => return Err(error.into()),
            }
        };
        let copied = if method == ImportMethod::Copy {
            let mut output = storage::temporary(staging.path())?;
            let fingerprint = storage::fingerprint(source, Some(output.as_file_mut()))?;
            storage::publish(output, &staged, false)?;
            Some(fingerprint)
        } else {
            None
        };
        // Keep this baseline across hashing, protection and publication. Reopening
        // after hashing could hide intervening edits to the hard-linked source.
        let content = ContentFile::open(&staged)?;
        let fingerprint = match copied {
            Some(fingerprint) => fingerprint,
            None => storage::fingerprint(&staged, None)?,
        };
        original.validate()?;
        content.validate()?;
        if fingerprint.id != id {
            return Err(CacheError::SourceChanged(source.to_owned()));
        }
        let (data, _) = self.paths(id);
        let parent = data.parent().expect("data has a parent");
        fs::create_dir_all(parent).at("create cache shard", parent)?;
        files::protect_file_contents(&staged).at("protect cached file contents", &staged)?;
        original.validate()?;
        content.validate()?;
        // Copied bytes were flushed before staging publication. A hard link
        // publishes existing bytes; a read-only Windows handle cannot flush them.
        match storage::publish_staged(&staged, &data) {
            Ok(()) => Ok(Some(method)),
            Err(CacheError::Io { source: error, .. })
                if error.kind() == io::ErrorKind::AlreadyExists =>
            {
                if self.reuse_data(id)? {
                    Ok(None)
                } else {
                    Err(CacheError::SourceChanged(data))
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Name existence alone is not enough to accept a concurrent publication.
    fn reuse_data(&self, id: CacheId) -> Result<bool, CacheError> {
        let (path, _) = self.paths(id);
        match storage::exists(&path).at("inspect existing cache data", &path)? {
            None => Ok(false),
            Some(metadata) if metadata.is_file() => {
                if ContentFile::open(&path)?.id()? != id {
                    return Err(Self::invalid(id));
                }
                Ok(true)
            }
            Some(_) => Err(Self::invalid(id)),
        }
    }

    pub(super) fn rebuild_index(&self) -> Result<usize, CacheError> {
        let mut entries = Vec::new();
        for id in self.ids()? {
            let (path, _) = self.paths(id);
            let result = (|| {
                let mut content = ContentFile::open(&path)?;
                if content.id()? != id {
                    return Err(Self::invalid(id));
                }
                content.quick_hash()
            })();
            match result {
                Ok(quick) => entries.push((quick, id)),
                Err(error) => {
                    tracing::debug!(%id, %error, "Skipping invalid/missing data during index rebuild")
                }
            }
        }
        index::replace(&self.root, &entries)?;
        Ok(entries.len())
    }
}
