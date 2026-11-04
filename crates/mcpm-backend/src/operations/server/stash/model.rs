//! Transport snapshots projected from stash and backup accessors, independent of storage codecs.

use std::{num::NonZeroUsize, path::PathBuf};

use mcpm_exec::error::PortableError;
use mcpm_lib::local::{mc::server::stash, mcmod::backup};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved snapshot; timestamps use RFC3339 UTC and paths stay on the owning host.
#[derive(Debug, Serialize, Deserialize)]
pub struct StashSnapshot {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
    pub backup_id: Uuid,
}
impl From<&stash::Stash> for StashSnapshot {
    fn from(stash: &stash::Stash) -> Self {
        Self {
            id: stash.id(),
            name: stash.name().into(),
            created_at: stash.created_at().to_rfc3339(),
            backup_id: stash.backup_id(),
        }
    }
}

/// Explicit selector; names are never inferred to be UUIDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Selector {
    Name(String),
    Id(Uuid),
}
impl From<&Selector> for stash::Selector {
    fn from(selector: &Selector) -> Self {
        match selector {
            Selector::Name(name) => Self::Name(name.clone()),
            Selector::Id(id) => Self::Id(*id),
        }
    }
}
impl From<stash::Selector> for Selector {
    fn from(selector: stash::Selector) -> Self {
        match selector {
            stash::Selector::Name(name) => Self::Name(name),
            stash::Selector::Id(id) => Self::Id(id),
        }
    }
}

/// File work performed by the host, without terminal policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: Stage,
    pub completed: u64,
    pub total: u64,
}

/// The current file-processing stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Checksum,
    Cache,
    Verify,
    Transfer,
}
impl From<backup::Stage> for Stage {
    fn from(stage: backup::Stage) -> Self {
        match stage {
            backup::Stage::Checksum => Self::Checksum,
            backup::Stage::Cache => Self::Cache,
            backup::Stage::Verify => Self::Verify,
            backup::Stage::Transfer => Self::Transfer,
        }
    }
}

/// Save one new snapshot and advance HEAD.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateInput {
    pub path: PathBuf,
    pub name: String,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum CreateEvent {
    Progress(ProgressEvent),
    Created(StashSnapshot),
}

/// Restore a snapshot; force permits discarding dirty mods, not invalid backup data.
#[derive(Debug, Serialize, Deserialize)]
pub struct SwitchInput {
    pub path: PathBuf,
    pub selector: Selector,
    pub force: bool,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum SwitchEvent {
    Progress(ProgressEvent),
    Switched { stash: StashSnapshot, changed: bool },
}

/// Compare the host workspace to its HEAD baseline.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusInput {
    pub path: PathBuf,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum StatusEvent {
    Progress(ProgressEvent),
    Status(StatusComparison),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusComparison {
    pub head: Option<StashSnapshot>,
    pub comparison: Option<WorkspaceComparison>,
}
impl From<&stash::Status> for StatusComparison {
    fn from(status: &stash::Status) -> Self {
        Self {
            head: status.head.as_ref().map(Into::into),
            comparison: status.comparison.as_ref().map(Into::into),
        }
    }
}

/// Semantic checksum comparison retaining both sides of changed files.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceComparison {
    pub expected: String,
    pub actual: String,
    pub differences: Vec<Difference>,
}
impl From<&backup::Comparison> for WorkspaceComparison {
    fn from(comparison: &backup::Comparison) -> Self {
        Self {
            expected: comparison.expected().to_string(),
            actual: comparison.actual().to_string(),
            differences: comparison.differences().iter().map(Into::into).collect(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Difference {
    Missing {
        expected: FileEntry,
    },
    Unexpected {
        actual: FileEntry,
    },
    Changed {
        expected: FileEntry,
        actual: FileEntry,
    },
}
impl From<&backup::FileDifference> for Difference {
    fn from(difference: &backup::FileDifference) -> Self {
        match difference {
            backup::FileDifference::Missing { expected } => Self::Missing {
                expected: expected.into(),
            },
            backup::FileDifference::Unexpected { actual } => Self::Unexpected {
                actual: actual.into(),
            },
            backup::FileDifference::Changed { expected, actual } => Self::Changed {
                expected: expected.into(),
                actual: actual.into(),
            },
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub blake3: String,
}
impl From<&backup::FileRecord> for FileEntry {
    fn from(file: &backup::FileRecord) -> Self {
        Self {
            path: file.path().into(),
            size: file.size(),
            blake3: file.blake3().to_string(),
        }
    }
}

/// Lightweight inventory; individual index failures do not hide other entries.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListInput {
    pub path: PathBuf,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ListOutput {
    pub head: Result<Option<Uuid>, PortableError>,
    pub entries: Vec<Result<EntrySnapshot, PortableError>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct EntrySnapshot {
    pub stash: StashSnapshot,
    pub backup_path: String,
}

/// Ordered selection with optional manifest loading.
#[derive(Debug, Serialize, Deserialize)]
pub struct InfoInput {
    pub path: PathBuf,
    pub selectors: Vec<Selector>,
    pub include_manifest: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct InfoOutput {
    pub head: Result<Option<Uuid>, PortableError>,
    pub entries: Vec<Result<DetailedEntry, PortableError>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct DetailedEntry {
    pub stash: StashSnapshot,
    pub backup_path: String,
    pub manifest: Option<ManifestSnapshot>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestSnapshot {
    pub checksum: String,
    pub files: Vec<FileRecord>,
}
impl From<&backup::Manifest> for ManifestSnapshot {
    fn from(manifest: &backup::Manifest) -> Self {
        Self {
            checksum: manifest.checksum().to_string(),
            files: manifest
                .files()
                .iter()
                .map(|file| FileRecord {
                    name: file.path().into(),
                    size: file.size(),
                    blake3: file.blake3().to_string(),
                })
                .collect(),
        }
    }
}
/// File sizes retain the library's platform-independent u64 range.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileRecord {
    pub name: String,
    pub size: u64,
    pub blake3: String,
}

/// Rename one snapshot without modifying its identity or HEAD.
#[derive(Debug, Serialize, Deserialize)]
pub struct RenameInput {
    pub path: PathBuf,
    pub selector: Selector,
    pub new_name: String,
}

/// One batch is preflighted before any deletion is performed.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteInput {
    pub path: PathBuf,
    pub selectors: Vec<Selector>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct DeletionResult {
    pub selector: Selector,
    pub result: Result<StashSnapshot, PortableError>,
}

/// Sequential verification retains the first selector for each resolved UUID.
#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyInput {
    pub path: PathBuf,
    pub selectors: Vec<Selector>,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum VerifyEvent {
    Progress(ProgressEvent),
    HeadFailed(PortableError),
    Item {
        selector: Selector,
        result: Result<StashSnapshot, PortableError>,
    },
}
