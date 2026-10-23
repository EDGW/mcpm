//! Cache observations and independent metadata/export/remove operations.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    CACHE_FORMAT_VERSION, CacheDigest, CacheEntry, CacheError, CacheId, CacheStatus,
    CacheUnavailableReason, ExportMethod, ExportMode, MOD_METADATA_VERSION, MetadataRefresh,
    VerificationMode, content::ContentFile, error::IoContext, index, storage,
};
use crate::local::{
    files,
    mcmod::{ModEntry, ModEntryName, ModMeta, cached_meta::CachedModMeta},
    modloader::{LoaderId, ModLoaderMeta},
};

#[derive(Debug)]
pub(super) struct Store {
    pub root: PathBuf,
}

impl Store {
    /// Observes structural availability without hashing the complete data file.
    /// This never creates a missing root, repairs objects or runs loaders.
    pub(super) fn get(&self, id: CacheId) -> CacheEntry {
        self.inspect(id)
    }

    /// Lists both complete objects and orphaned halves, ignoring temporary files.
    /// Per-object problems are states; directory enumeration failures are errors.
    pub(super) fn list(&self) -> Result<Vec<CacheEntry>, CacheError> {
        Ok(self.ids()?.into_iter().map(|id| self.inspect(id)).collect())
    }

    /// Enumerates names without reading metadata JSON or cached contents.
    pub(super) fn ids(&self) -> Result<Vec<CacheId>, CacheError> {
        let shards = match fs::read_dir(&self.root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            result => result.at("list cache directory", &self.root)?,
        };
        let mut ids = BTreeSet::new();
        for shard in shards {
            let shard = shard.at("read cache shard entry", &self.root)?;
            let name = shard.file_name();
            let Some(_) = name.to_str().filter(|name| {
                name.len() == 2
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }) else {
                continue;
            };
            if !shard
                .file_type()
                .at("inspect cache shard", &shard.path())?
                .is_dir()
            {
                continue;
            }
            for entry in fs::read_dir(shard.path()).at("list cache shard", &shard.path())? {
                let entry = entry.at("read cache object entry", &shard.path())?;
                if let Some(id) = self.id_from_path(&entry.path()) {
                    ids.insert(id);
                }
            }
        }
        Ok(ids.into_iter().collect())
    }

    /// Checks structure, optionally rehashing bytes. Never repairs or deletes data.
    pub(super) fn verify(&self, id: CacheId, mode: VerificationMode) -> CacheStatus {
        let result = (|| {
            let entry = self.inspect(id);
            if mode == VerificationMode::Content
                && matches!(
                    entry.status,
                    CacheStatus::Available
                        | CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
                )
            {
                let actual = storage::fingerprint(&entry.data_path, None)?;
                if actual.id != id
                    || entry
                        .digest
                        .as_ref()
                        .is_some_and(|digest| !actual.matches(digest))
                {
                    return Ok(CacheStatus::Unavailable(
                        CacheUnavailableReason::InvalidData,
                    ));
                }
            }
            Ok::<_, CacheError>(entry.status)
        })();
        result.unwrap_or_else(|error| {
            CacheStatus::Unavailable(CacheUnavailableReason::UnexpectedError(error.to_string()))
        })
    }

    /// Identifies selected loaders without publishing; pins and validates the source.
    /// Returns the fingerprint of the bytes from which the results were produced.
    pub(super) fn identify_metadata(
        &self,
        id: CacheId,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
        mode: MetadataRefresh,
    ) -> Result<(String, HashMap<LoaderId, Option<ModMeta>>), CacheError> {
        let digest = self.prepare_metadata(id)?;
        let (data_path, _) = self.paths(id);
        let source = ContentFile::open(&data_path)?;
        let entry = ModEntry::from_parts(
            data_path,
            ModEntryName::new("cache.jar").expect("fixed JAR name is valid"),
        );
        let mut selected: Vec<_> = loaders.values().collect();
        selected.sort_by_key(|loader| loader.id());
        let mut metadata = HashMap::new();
        for loader in selected {
            let key = loader.id().as_str();
            if mode == MetadataRefresh::MissingOrOutdated && digest.loaders.contains_key(key) {
                continue;
            }
            let meta =
                loader
                    .instance()
                    .try_identify(&entry)
                    .map_err(|source| CacheError::Identify {
                        loader: key.to_owned(),
                        source,
                    })?;
            metadata.insert(loader.id().clone(), meta);
        }
        source.validate()?;
        Ok((digest.sha256, metadata))
    }

    /// Internal trusted write path. Never expose or use outside this crate under any
    /// circumstances: associating unrelated metadata with bytes destroys consistency.
    /// The caller must establish result provenance. Concurrent valid snapshots may overwrite.
    pub(super) fn write_metadata(
        &self,
        id: CacheId,
        expected_sha256: &str,
        metadata: &HashMap<LoaderId, Option<ModMeta>>,
    ) -> Result<(CacheEntry, bool), CacheError> {
        let mut digest = self.prepare_metadata(id)?;
        if digest.sha256 != expected_sha256 {
            return Err(Self::invalid(id));
        }
        if metadata.is_empty() {
            return Ok((self.inspect(id), false));
        }
        for (loader, meta) in metadata {
            let snapshot = meta.as_ref().map(CachedModMeta::capture).transpose()?;
            if snapshot
                .as_ref()
                .is_some_and(|meta| meta.loader != loader.as_str())
            {
                return Err(Self::invalid(id));
            }
            digest.loaders.insert(loader.as_str().to_owned(), snapshot);
        }
        if !digest.valid_metadata() {
            return Err(Self::invalid(id));
        }
        let (_, digest_path) = self.paths(id);
        let changed = storage::write_digest(&digest_path, &digest, true)?;
        Ok((self.inspect(id), changed))
    }

    /// Exports a regular file; verifies stored fingerprints when a digest exists.
    /// The returned method makes shared hard-link ownership explicit.
    pub(super) fn export(
        &self,
        id: CacheId,
        destination: &Path,
        mode: ExportMode,
    ) -> Result<ExportMethod, CacheError> {
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let target_parent = fs::canonicalize(parent).at("resolve export parent", parent)?;
        let cache_root = fs::canonicalize(&self.root).at("resolve cache directory", &self.root)?;
        if target_parent.starts_with(cache_root) {
            return Err(CacheError::ExportIntoCache(destination.to_owned()));
        }
        let entry = self.inspect(id);
        let digest = Self::content_descriptor(&entry)?;
        if mode != ExportMode::Copy {
            let actual = storage::fingerprint(&entry.data_path, None)?;
            if actual.id != id || digest.is_some_and(|digest| !actual.matches(digest)) {
                return Err(Self::invalid(id));
            }
            match files::hard_link(&entry.data_path, destination) {
                Ok(()) => {
                    let result = storage::fingerprint(destination, None).and_then(|published| {
                        if published.id != id
                            || digest.is_some_and(|digest| !published.matches(digest))
                        {
                            Err(Self::invalid(id))
                        } else {
                            Ok(ExportMethod::HardLink)
                        }
                    });
                    if result.is_err()
                        && let Err(error) = storage::remove_if_exists(destination)
                    {
                        tracing::warn!(path = %destination.display(), %error, "Failed to remove unverified exported link");
                    }
                    return result;
                }
                Err(error)
                    if mode == ExportMode::PreferHardLink && error.allows_copy_fallback() => {}
                Err(error) => return Err(error.into()),
            }
        }
        let mut temp = storage::temporary(parent)?;
        let actual = storage::fingerprint(&entry.data_path, Some(temp.as_file_mut()))?;
        if actual.id != id || digest.is_some_and(|digest| !actual.matches(digest)) {
            return Err(Self::invalid(id));
        }
        storage::publish(temp, destination, false)?;
        Ok(ExportMethod::Copy)
    }

    /// Removes only the cache's two names. Other hard links remain valid.
    /// Returns false when neither existed; errors can leave a partially removed object.
    pub(super) fn remove(&self, id: CacheId) -> Result<bool, CacheError> {
        let (data, digest) = self.paths(id);
        let removed_data = storage::remove_if_exists(&data)?;
        let removed_digest = storage::remove_if_exists(&digest)?;
        if let Err(error) = index::remove(&self.root, id) {
            tracing::debug!(%id, %error, "Skipping optional quick-index removal");
        }
        Ok(removed_data || removed_digest)
    }

    /// Parses only exact canonical object names at their matching shard location.
    pub(super) fn id_from_path(&self, path: &Path) -> Option<CacheId> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let mut parts = relative.components();
        let prefix = parts.next()?.as_os_str().to_str()?;
        let filename = parts.next()?.as_os_str().to_str()?;
        if parts.next().is_some() {
            return None;
        }
        let stem = filename
            .strip_suffix(".data.json")
            .or_else(|| filename.strip_suffix(".dat"))?;
        let id: CacheId = stem.parse().ok()?;
        let canonical = id.to_string();
        (stem == canonical && prefix == &canonical[..2]).then_some(id)
    }

    pub(super) fn paths(&self, id: CacheId) -> (PathBuf, PathBuf) {
        let id = id.to_string();
        let shard = self.root.join(&id[..2]);
        (
            shard.join(format!("{id}.dat")),
            shard.join(format!("{id}.data.json")),
        )
    }

    /// Validates existing content and initializes only the in-memory descriptor.
    /// Used before identification and again before committing its results.
    fn prepare_metadata(&self, id: CacheId) -> Result<CacheDigest, CacheError> {
        let entry = self.inspect(id);
        let existing = Self::content_descriptor(&entry)?;
        let fingerprint = storage::fingerprint(&entry.data_path, None)?;
        let mut digest = existing.cloned().unwrap_or_else(|| CacheDigest {
            version: CACHE_FORMAT_VERSION,
            blake3: id.to_string(),
            size: fingerprint.size,
            sha256: fingerprint.sha256.clone(),
            metadata_version: MOD_METADATA_VERSION,
            loaders: BTreeMap::new(),
        });
        if fingerprint.id != id || !fingerprint.matches(&digest) {
            return Err(Self::invalid(id));
        }
        if digest.metadata_version != MOD_METADATA_VERSION {
            digest.loaders.clear();
            digest.metadata_version = MOD_METADATA_VERSION;
        } else if entry.digest.is_some() && !digest.valid_metadata() {
            return Err(Self::invalid(id));
        }
        Ok(digest)
    }

    fn inspect(&self, id: CacheId) -> CacheEntry {
        let (data_path, digest_path) = self.paths(id);
        let mut entry = CacheEntry {
            id,
            data_path,
            digest_path,
            status: CacheStatus::NotFound,
            digest: None,
        };
        let result = (|| {
            let data =
                storage::exists(&entry.data_path).at("inspect cache data", &entry.data_path)?;
            let digest = storage::exists(&entry.digest_path)
                .at("inspect cache digest", &entry.digest_path)?;
            let Some(data) = data else {
                return Ok(if digest.is_none() {
                    CacheStatus::NotFound
                } else {
                    CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
                });
            };
            if !data.is_file() {
                return Ok(CacheStatus::Unavailable(
                    CacheUnavailableReason::InvalidData,
                ));
            }
            let Some(digest_metadata) = digest else {
                return Ok(CacheStatus::Unavailable(
                    CacheUnavailableReason::MissingDigest,
                ));
            };
            if !digest_metadata.is_file() {
                return Ok(CacheStatus::Unavailable(
                    CacheUnavailableReason::InvalidData,
                ));
            }
            let digest = match storage::read_digest(&entry.digest_path) {
                Ok(digest) => digest,
                Err(CacheError::Digest { .. }) => {
                    return Ok(CacheStatus::Unavailable(
                        CacheUnavailableReason::InvalidData,
                    ));
                }
                Err(error) => return Err(error),
            };
            let valid = digest.blake3 == id.to_string()
                && digest.valid_content_descriptor()
                && digest.valid_metadata()
                && digest.size == data.len();
            entry.digest = Some(digest);
            Ok::<_, CacheError>(if valid {
                CacheStatus::Available
            } else {
                CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
            })
        })();
        entry.status = result.unwrap_or_else(|error| {
            CacheStatus::Unavailable(CacheUnavailableReason::UnexpectedError(error.to_string()))
        });
        entry
    }

    /// File access can proceed without identification, or with stale metadata,
    /// but cannot ignore an existing malformed content descriptor or I/O failure.
    fn content_descriptor(entry: &CacheEntry) -> Result<Option<&CacheDigest>, CacheError> {
        match &entry.status {
            CacheStatus::NotFound => Err(CacheError::NotFound(entry.id)),
            CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest) => Ok(None),
            CacheStatus::Unavailable(reason @ CacheUnavailableReason::UnexpectedError(_)) => {
                Err(CacheError::Unavailable {
                    id: entry.id,
                    reason: reason.clone(),
                })
            }
            _ => entry
                .digest
                .as_ref()
                .filter(|digest| digest.valid_content_descriptor())
                .map(Some)
                .ok_or_else(|| Self::invalid(entry.id)),
        }
    }

    pub(super) fn invalid(id: CacheId) -> CacheError {
        CacheError::Unavailable {
            id,
            reason: CacheUnavailableReason::InvalidData,
        }
    }
}
