//! Cache operation inputs and owned observations, independent of on-disk snapshot formats.

use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

use mcpm_exec::error::PortableError;
use mcpm_lib::{
    local::{
        cache::{
            self,
            hash::Blake3Hash,
            mods::{CacheStatus, CacheUnavailableReason, ModsCacheSelection},
        },
        mcmod::CachedModMeta,
    },
    source::FileMatch,
};
use serde::{Deserialize, Serialize};

/// Cache location and explicit permission to bypass stash management protection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheInput {
    pub selection: ModsCacheSelection,
    pub no_protect_stash: bool,
}

/// Explicit keys retain input order; All selects the complete inventory in key order.
#[derive(Debug, Serialize, Deserialize)]
pub enum Selection {
    Ids(Vec<Blake3Hash>),
    All,
}
impl Selection {
    pub(super) fn resolve(
        self,
        cache: &cache::mods::ModsCacheDir,
    ) -> anyhow::Result<Vec<Blake3Hash>> {
        match self {
            Self::Ids(ids) => {
                let mut seen = std::collections::HashSet::new();
                Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
            }
            Self::All => Ok(cache.scan()?.into_iter().map(|entry| entry.key).collect()),
        }
    }
}

/// The execution host's initialized directory and storage version.
#[derive(Debug, Serialize, Deserialize)]
pub struct InitOutput {
    pub directory: PathBuf,
    pub version: usize,
}

/// Full diagnostic inventory, including orphan metadata and invalid content types.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListOutput {
    pub directory: PathBuf,
    pub entries: Vec<ListEntry>,
}
/// A key is retained even when its filesystem observation fails.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListEntry {
    pub key: Blake3Hash,
    /// Content path formatted on the owning host, including for missing objects.
    pub file: PathBuf,
    pub inspection: Result<Inspection, PortableError>,
}
/// Filesystem structure only; no content hashing or metadata decoding.
#[derive(Debug, Serialize, Deserialize)]
pub struct Inspection {
    pub content: Option<FileKind>,
    pub metadata: Vec<MetadataEntry>,
}
impl From<cache::CacheInspection> for Inspection {
    fn from(value: cache::CacheInspection) -> Self {
        Self {
            content: value.content.map(Into::into),
            metadata: value
                .metadata
                .into_iter()
                .map(|entry| MetadataEntry {
                    name: entry.name,
                    kind: entry.kind.into(),
                })
                .collect(),
        }
    }
}
/// A cache-owned metadata name and the actual type of its filesystem entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub name: String,
    pub kind: FileKind,
}
/// Native filesystem entry type, not a Mod or metadata format.
#[derive(Debug, Serialize, Deserialize)]
pub enum FileKind {
    File { size: u64 },
    Directory,
    SymbolicLink,
    Other,
}
impl From<cache::CacheFileKind> for FileKind {
    fn from(value: cache::CacheFileKind) -> Self {
        match value {
            cache::CacheFileKind::File { size } => Self::File { size },
            cache::CacheFileKind::Directory => Self::Directory,
            cache::CacheFileKind::SymbolicLink => Self::SymbolicLink,
            cache::CacheFileKind::Other => Self::Other,
        }
    }
}

/// Data requirements, not terminal modes or output columns.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct ReadOptions {
    pub sha256: bool,
    pub metadata: bool,
    pub metadata_details: bool,
}
/// Select entries and request only the saved data needed by the caller.
#[derive(Debug, Serialize, Deserialize)]
pub struct InfoInput {
    pub cache: CacheInput,
    pub selection: Selection,
    pub options: ReadOptions,
}

/// Content availability; Available alone is not a full hash verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentStatus {
    Available,
    NotFound,
    Unavailable,
}
impl ContentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A typed metadata observation; availability is separate from content integrity.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataState {
    pub status: ContentStatus,
    pub reason: Option<String>,
}
impl From<&CacheStatus> for MetadataState {
    fn from(value: &CacheStatus) -> Self {
        match value {
            CacheStatus::Available => Self {
                status: ContentStatus::Available,
                reason: None,
            },
            CacheStatus::NotFound => Self {
                status: ContentStatus::NotFound,
                reason: None,
            },
            CacheStatus::Unavailable(reason) => Self {
                status: ContentStatus::Unavailable,
                reason: Some(match reason {
                    CacheUnavailableReason::MissingDigest => "missing_digest".into(),
                    CacheUnavailableReason::InvalidData => "invalid_data".into(),
                    CacheUnavailableReason::UnexpectedError(error) => error.clone(),
                }),
            },
        }
    }
}
/// Owned, read-only content observation with independently observed metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: Blake3Hash,
    pub file: PathBuf,
    pub status: ContentStatus,
    pub reason: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub metadatas: Vec<Metadata>,
}
/// Known metadata snapshots or uninterpreted names.
#[derive(Debug, Serialize, Deserialize)]
pub enum Metadata {
    Meta(MetaSnapshot),
    Ident(IdentSnapshot),
    Other(String),
}
impl Metadata {
    pub fn name(&self) -> &str {
        match self {
            Self::Meta(_) => "meta",
            Self::Ident(_) => "ident",
            Self::Other(name) => name,
        }
    }
}
/// Only checked loaders are present; None is successful non-recognition.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetaSnapshot {
    pub state: MetadataState,
    pub identifications: BTreeMap<String, Option<CachedModMeta>>,
}
impl From<&cache::mods::meta::CacheEntry> for MetaSnapshot {
    fn from(value: &cache::mods::meta::CacheEntry) -> Self {
        use cache::mods::meta::CachedIdentification;
        Self {
            state: value.status().into(),
            identifications: value
                .digest()
                .filter(|_| value.status() == &CacheStatus::Available)
                .into_iter()
                .flat_map(|digest| digest.identifications())
                .filter_map(|(loader, result)| match result {
                    CachedIdentification::Missing => None,
                    CachedIdentification::Unrecognized => Some((loader.to_owned(), None)),
                    CachedIdentification::Recognized(meta) => {
                        Some((loader.to_owned(), Some(meta.clone())))
                    }
                })
                .collect(),
        }
    }
}
/// Saved positive source matches, with no provider requests or transient outcomes.
#[derive(Debug, Serialize, Deserialize)]
pub struct IdentSnapshot {
    pub state: MetadataState,
    pub matches: BTreeMap<String, FileMatch>,
}
impl From<&cache::mods::ident::CacheIdentInfo> for IdentSnapshot {
    fn from(value: &cache::mods::ident::CacheIdentInfo) -> Self {
        Self {
            state: value.status().into(),
            matches: value
                .matches()
                .map(|(source, file)| (source.to_owned(), file.clone()))
                .collect(),
        }
    }
}

/// Requested filesystem transfer strategy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransferPreference {
    Copy,
    HardLink,
    PreferHardLink,
}
impl From<TransferPreference> for cache::CacheLinkPreference {
    fn from(value: TransferPreference) -> Self {
        match value {
            TransferPreference::Copy => Self::Copy,
            TransferPreference::HardLink => Self::HardLink,
            TransferPreference::PreferHardLink => Self::PreferHardLink,
        }
    }
}
/// Transfer actually performed; reused imports have no method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransferMethod {
    Copy,
    HardLink,
}
impl From<cache::TransferMethod> for TransferMethod {
    fn from(value: cache::TransferMethod) -> Self {
        match value {
            cache::TransferMethod::Copy => Self::Copy,
            cache::TransferMethod::HardLink => Self::HardLink,
        }
    }
}
impl From<TransferMethod> for cache::TransferMethod {
    fn from(value: TransferMethod) -> Self {
        match value {
            TransferMethod::Copy => Self::Copy,
            TransferMethod::HardLink => Self::HardLink,
        }
    }
}
/// Ordered execution-host source paths; import stores content only.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportInput {
    pub cache: CacheInput,
    pub paths: Vec<PathBuf>,
    pub mode: TransferPreference,
}
/// One completed import, emitted before attempting the next source.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportReceipt {
    pub id: Blake3Hash,
    pub file: PathBuf,
    pub published: bool,
    pub method: Option<TransferMethod>,
}
/// Export one object to ordered execution-host destinations.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportInput {
    pub cache: CacheInput,
    pub id: Blake3Hash,
    pub destinations: Vec<PathBuf>,
    pub mode: TransferPreference,
}
/// One published export; subsequent failures do not roll it back.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportReceipt {
    pub id: Blake3Hash,
    pub destination: PathBuf,
    pub method: TransferMethod,
}
/// Remove only explicit keys, including a previously confirmed inventory.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveInput {
    pub cache: CacheInput,
    /// Frozen execution directory from List; the original selection is still checked for stash aliases.
    pub directory: Option<PathBuf>,
    pub ids: Vec<Blake3Hash>,
}
/// Result of deleting one cache-owned key, without affecting external links.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemovalReceipt {
    pub id: Blake3Hash,
    pub removed: bool,
}
/// Content-only verification and optional cleanup policy.
#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyInput {
    pub cache: CacheInput,
    pub selection: Selection,
    pub parallelism: NonZeroUsize,
    pub structure_only: bool,
    pub autoremove: bool,
}
/// Selection size, ordered results and final aggregate statistics.
#[derive(Debug, Serialize, Deserialize)]
pub enum VerifyEvent {
    Selected { total: usize },
    Item(VerificationItem),
    Finished(VerificationSummary),
}
/// Verification and cleanup errors are independent and preserve their cause trees.
#[derive(Debug, Serialize, Deserialize)]
pub struct VerificationItem {
    pub id: Blake3Hash,
    pub status: ContentStatus,
    pub error: Option<PortableError>,
    pub removal: Option<Result<bool, PortableError>>,
}
/// All completed verifications, including failures that were successfully removed.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub available: usize,
    pub failed: usize,
    pub removal: Option<RemovalSummary>,
}
/// Cleanup statistics for failed verification entries only.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RemovalSummary {
    pub removed: usize,
    pub absent: usize,
    pub failed: usize,
}
