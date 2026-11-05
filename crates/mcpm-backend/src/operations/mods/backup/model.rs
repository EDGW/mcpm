//! Owned backup observations for transport; manifest parsing and comparison remain library-owned.

use std::path::{Path, PathBuf};

use mcpm_lib::local::{files::hash::Blake3Hash, mcmod::backup};
use serde::{Deserialize, Serialize};

use crate::operations::cache::TransferMethod;
pub use backup::FileRecord;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    files: Vec<FileRecord>,
    checksum: Blake3Hash,
}
impl Manifest {
    pub fn files(&self) -> &[FileRecord] {
        &self.files
    }
    pub fn checksum(&self) -> Blake3Hash {
        self.checksum
    }
}
impl From<&backup::Manifest> for Manifest {
    fn from(value: &backup::Manifest) -> Self {
        Self {
            files: value.files().to_vec(),
            checksum: value.checksum(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupOperation {
    Create,
    CreateCacheOnly,
    Apply,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileSource {
    Backup,
    Cache,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileDisposition {
    CacheOnly,
    Transferred {
        source: FileSource,
        method: TransferMethod,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Outcome {
    operation: BackupOperation,
    destination: PathBuf,
    manifest: Manifest,
    manifest_size: u64,
    dispositions: Vec<FileDisposition>,
}
impl Outcome {
    pub fn operation(&self) -> BackupOperation {
        self.operation
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn manifest_size(&self) -> u64 {
        self.manifest_size
    }
    pub fn files(&self) -> impl Iterator<Item = (&FileRecord, FileDisposition)> {
        self.manifest
            .files
            .iter()
            .zip(self.dispositions.iter().copied())
    }
}
impl From<&backup::Outcome> for Outcome {
    fn from(value: &backup::Outcome) -> Self {
        Self {
            operation: match value.operation() {
                backup::Operation::Create => BackupOperation::Create,
                backup::Operation::CreateCacheOnly => BackupOperation::CreateCacheOnly,
                backup::Operation::Apply => BackupOperation::Apply,
            },
            destination: value.destination().into(),
            manifest: value.manifest().into(),
            manifest_size: value.manifest_size(),
            dispositions: value
                .files()
                .map(|(_, disposition)| match disposition {
                    backup::FileDisposition::CacheOnly => FileDisposition::CacheOnly,
                    backup::FileDisposition::Transferred { source, method } => {
                        FileDisposition::Transferred {
                            source: match source {
                                backup::FileSource::Backup => FileSource::Backup,
                                backup::FileSource::Cache => FileSource::Cache,
                            },
                            method: method.into(),
                        }
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
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
impl From<&backup::VerifiedBackup> for VerifiedBackup {
    fn from(value: &backup::VerifiedBackup) -> Self {
        Self {
            directory: value.directory().into(),
            manifest: value.manifest().into(),
            manifest_size: value.manifest_size(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DirectoryChecksum {
    directory: PathBuf,
    manifest: Manifest,
}
impl DirectoryChecksum {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}
impl From<&backup::DirectoryChecksum> for DirectoryChecksum {
    fn from(value: &backup::DirectoryChecksum) -> Self {
        Self {
            directory: value.directory().into(),
            manifest: value.manifest().into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Comparison {
    expected: Blake3Hash,
    actual: Blake3Hash,
    differences: Vec<FileDifference>,
}
impl Comparison {
    pub fn matches(&self) -> bool {
        self.expected == self.actual
    }
    pub fn expected(&self) -> Blake3Hash {
        self.expected
    }
    pub fn actual(&self) -> Blake3Hash {
        self.actual
    }
    pub fn differences(&self) -> &[FileDifference] {
        &self.differences
    }
}
impl From<&backup::Comparison> for Comparison {
    fn from(value: &backup::Comparison) -> Self {
        Self {
            expected: value.expected(),
            actual: value.actual(),
            differences: value
                .differences()
                .iter()
                .map(|difference| match difference {
                    backup::FileDifference::Missing { expected } => FileDifference::Missing {
                        expected: expected.clone(),
                    },
                    backup::FileDifference::Unexpected { actual } => FileDifference::Unexpected {
                        actual: actual.clone(),
                    },
                    backup::FileDifference::Changed { expected, actual } => {
                        FileDifference::Changed {
                            expected: expected.clone(),
                            actual: actual.clone(),
                        }
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileDifference {
    Missing {
        expected: FileRecord,
    },
    Unexpected {
        actual: FileRecord,
    },
    Changed {
        expected: FileRecord,
        actual: FileRecord,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Stage {
    Checksum,
    Cache,
    Verify,
    Transfer,
}
impl From<backup::Stage> for Stage {
    fn from(value: backup::Stage) -> Self {
        match value {
            backup::Stage::Checksum => Self::Checksum,
            backup::Stage::Cache => Self::Cache,
            backup::Stage::Verify => Self::Verify,
            backup::Stage::Transfer => Self::Transfer,
        }
    }
}
impl From<Stage> for backup::Stage {
    fn from(value: Stage) -> Self {
        match value {
            Stage::Checksum => Self::Checksum,
            Stage::Cache => Self::Cache,
            Stage::Verify => Self::Verify,
            Stage::Transfer => Self::Transfer,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Progress {
    Progress {
        stage: Stage,
        completed: usize,
        total: usize,
    },
    StageDone(Stage),
    BeforeCommit,
}
impl From<backup::Event> for Progress {
    fn from(value: backup::Event) -> Self {
        match value {
            backup::Event::Progress {
                stage,
                completed,
                total,
            } => Self::Progress {
                stage: stage.into(),
                completed,
                total,
            },
            backup::Event::StageDone(stage) => Self::StageDone(stage.into()),
            backup::Event::BeforeCommit => Self::BeforeCommit,
        }
    }
}
impl From<Progress> for backup::Event {
    fn from(value: Progress) -> Self {
        match value {
            Progress::Progress {
                stage,
                completed,
                total,
            } => Self::Progress {
                stage: stage.into(),
                completed,
                total,
            },
            Progress::StageDone(stage) => Self::StageDone(stage.into()),
            Progress::BeforeCommit => Self::BeforeCommit,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PublicationEvent {
    Progress(Progress),
    Finished {
        outcome: Outcome,
        cache: Option<PathBuf>,
    },
}
