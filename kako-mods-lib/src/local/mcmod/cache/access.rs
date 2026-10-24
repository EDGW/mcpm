//! Read and write capabilities over best-effort atomic cache operations.

use std::{collections::HashMap, marker::PhantomData, ops::Deref, path::Path};

use super::{
    CacheEntry, CacheError, CacheId, CacheStatus, ExportMethod, ExportMode, ImportMode,
    ImportOutcome, MetadataRefresh, VerificationMode, operations::Store,
};
use crate::local::{
    mcmod::ModMeta,
    modloader::{LoaderId, ModLoaderMeta},
};

/// Read-only cache access, without a retained filesystem lock.
/// Returned observations and paths may change during concurrent mutations.
#[derive(Debug)]
pub struct CacheReader<'a> {
    pub(super) store: Store,
    pub(super) _owner: PhantomData<&'a super::ModsCache>,
}

impl CacheReader<'_> {
    /// Loader metadata observation; independent of source identification.
    pub fn metadata(&self, id: CacheId) -> CacheEntry {
        self.store.get(id)
    }

    /// Source-match observation; requires .dat, independently of loader metadata.
    pub fn ident_info(&self, id: CacheId) -> super::CacheIdentInfo {
        self.store.ident_info(id)
    }

    /// Checks file presence, descriptor structure and size without hashing bytes.
    pub fn get(&self, id: CacheId) -> CacheEntry {
        self.metadata(id)
    }

    /// Includes orphaned halves and unavailable objects; ignores temporary files.
    pub fn list(&self) -> Result<Vec<CacheEntry>, CacheError> {
        self.store.list()
    }

    /// Checks structure and optionally hashes contents, without modifying objects.
    pub fn verify(&self, id: CacheId, mode: VerificationMode) -> CacheStatus {
        self.store.verify(id, mode)
    }

    /// Writes contents to an absent target outside the cache directory;
    /// does not create parent directories or allow shared access to mutate cache names.
    /// A missing digest does not prevent exporting a regular data file.
    /// Existing compatible content fingerprints are verified even with stale ModMeta.
    /// Malformed or incompatible content descriptors are rejected.
    /// Only hard-link capability failures permit PreferHardLink to fall back to copying.
    pub fn export(
        &self,
        id: CacheId,
        destination: &Path,
        mode: ExportMode,
    ) -> Result<ExportMethod, CacheError> {
        self.store.export(id, destination, mode)
    }
}

/// Write-capable access; mutations require &mut self but do not exclude other handles.
/// Each operation publishes independently. Concurrent updates may overwrite each other.
#[derive(Debug)]
pub struct CacheWriter<'a> {
    pub(super) reader: CacheReader<'a>,
}

impl CacheWriter<'_> {
    /// Copies and deduplicates a regular file by BLAKE3 without identifying it.
    /// Creates only .dat; MissingDigest persists until identification is performed.
    pub fn import(&mut self, source: &Path) -> Result<CacheId, CacheError> {
        self.import_with_mode(source, ImportMode::Copy)
            .map(|outcome| outcome.id)
    }

    /// Imports using the requested strategy, or reuses an existing content object.
    /// Hard links share content protection with the source. Only filesystem
    /// capability errors allow PreferHardLink to fall back to copying.
    pub fn import_with_mode(
        &mut self,
        source: &Path,
        mode: ImportMode,
    ) -> Result<ImportOutcome, CacheError> {
        self.reader.store.import(source, mode)
    }

    /// Publishes selected loader results together, only after all succeed.
    /// Initializes missing digests, including explicit unrecognized loader results.
    /// Broken content descriptors require reimport; stale metadata can be rebuilt.
    /// Concurrent snapshots may lose loader updates, but cannot mix content identities.
    pub fn refresh_metadata(
        &mut self,
        id: CacheId,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
        mode: MetadataRefresh,
    ) -> Result<CacheEntry, CacheError> {
        if loaders.is_empty() {
            return Ok(self.get(id));
        }
        let (sha256, metadata) = self.reader.store.identify_metadata(id, loaders, mode)?;
        // Never expose or use write_metadata outside this crate under any circumstances:
        // incorrect associations between bytes and metadata destroy cache consistency.
        // Publication rechecks the content fingerprint from which these results were identified.
        self.write_metadata(id, &sha256, &metadata)
            .map(|(entry, _)| entry)
    }

    /// Publishes a source-verified match after checking BLAKE3, size and SHA-1 against .dat.
    /// The source reference must use its identification_cache_id (all CurseForge uses "curseforge").
    /// The caller must obtain the match from a trusted Source result, not construct a
    /// guessed publication. Hash checks establish content correspondence, not provider provenance.
    /// Intended for client-side provider requests whose bytes are stored on another host.
    /// No caller-supplied loader metadata or missing data object is accepted.
    pub fn write_ident_info(
        &mut self,
        id: CacheId,
        matched: &crate::source::FileMatch,
    ) -> Result<super::CacheIdentInfo, CacheError> {
        self.reader.store.write_ident_info(id, matched)
    }

    /// Explicitly queries a source using verified cached bytes and publishes only a successful match.
    /// Never imports files. None/errors do not become persistent negative results.
    /// Source configurations sharing identification_cache_id reuse the same result.
    pub async fn refresh_ident_info(
        &mut self,
        id: CacheId,
        source: &dyn crate::source::Source,
        mode: MetadataRefresh,
    ) -> Result<super::CacheIdentInfo, CacheError> {
        self.reader.store.refresh_ident_info(id, source, mode).await
    }

    /// Atomically merges already identified loader results without running loaders.
    /// `None` records successful non-recognition; omitted loaders are preserved unless
    /// the metadata version changed. An empty map does not create a missing digest.
    /// `expected_sha256` must be the SHA-256 of the bytes that produced these results.
    /// Returns the observed entry and whether the digest was actually published.
    ///
    /// # Consistency contract
    ///
    /// Under no circumstances may this interface be exposed or used outside this crate.
    /// Incorrect use can destroy cache data consistency. Every caller must document
    /// this restriction at the call site and establish that all supplied results were
    /// produced from this object's exact bytes with the current identification rules.
    /// Hash and structural checks cannot establish that semantic relationship.
    pub(crate) fn write_metadata(
        &mut self,
        id: CacheId,
        expected_sha256: &str,
        metadata: &HashMap<LoaderId, Option<ModMeta>>,
    ) -> Result<(CacheEntry, bool), CacheError> {
        // Crate-internal only: never expose or use this outside the crate; incorrect
        // metadata provenance destroys cache consistency despite structural checks.
        self.reader
            .store
            .write_metadata(id, expected_sha256, metadata)
    }

    /// Rebuilds the optional imohash lookup table from current immutable data.
    /// Concurrent additions/removals may be absent or stale; lookup always validates candidates.
    pub fn rebuild_index(&mut self) -> Result<usize, CacheError> {
        self.reader.store.rebuild_index()
    }

    /// Removes cache names only. Other hard links survive. Missing objects return false.
    /// A filesystem error can leave a partially removed object.
    pub fn remove(&mut self, id: CacheId) -> Result<bool, CacheError> {
        self.reader.store.remove(id)
    }
}

impl<'a> Deref for CacheWriter<'a> {
    type Target = CacheReader<'a>;
    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}
