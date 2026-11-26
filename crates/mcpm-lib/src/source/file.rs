//! Installable remote files, dependency targets, and filtered file pagination.

use serde::{Deserialize, Serialize};
use url::Url;

use super::{ProjectRef, SourceId};

/// An installable file identified by a publication in its project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub reference: FileRef,
    pub name: String,
    /// Author-declared mod version; never inferred from names or filenames.
    pub version: Option<String>,
    pub release_type: ReleaseType,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub filename: String,
    pub size: u64,
    /// Some sources or authors do not expose direct downloads.
    pub download_url: Option<Url>,
    pub hashes: Vec<FileHash>,
    pub dependencies: Vec<FileDependency>,
}

/// Source-native publication ID: Modrinth Version ID or CurseForge File ID.
/// This is not necessarily an identifier of a physical file on the provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub String);

/// A file publication qualified by its owning project and configured source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub project: ProjectRef,
    pub id: FileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
    Unknown,
}

/// Hex-encoded checksums of the downloadable file, not provider fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algorithm", content = "value", rename_all = "snake_case")]
pub enum FileHash {
    Sha1(String),
    Sha512(String),
    Md5(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDependency {
    pub target: DependencyTarget,
    pub kind: DependencyKind,
}

/// Dependency declarations do not imply a version range or proven compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyTarget {
    Project(ProjectRef),
    File(FileRef),
    /// A filename-only declaration that cannot be resolved to a project.
    Filename(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
    Tool,
    Include,
}

/// AND-combined fields, OR within each list, applied before pagination; newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileQuery {
    /// Match any listed Minecraft version; empty means unrestricted.
    pub game_versions: Vec<String>,
    /// Match any of these loaders; empty means no loader filter.
    pub loaders: Vec<String>,
    pub release_type: Option<ReleaseType>,
    pub offset: u64,
    /// Must be positive. Sources may impose an upper bound.
    pub limit: u32,
}

impl Default for FileQuery {
    fn default() -> Self {
        Self {
            game_versions: Vec::new(),
            loaders: Vec::new(),
            release_type: None,
            offset: 0,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePage {
    pub files: Vec<ProjectFile>,
    pub offset: u64,
    /// Count of matching publications, when reported or computed by the source.
    pub total: Option<u64>,
}

/// A verified local-file match. The reference identifies its publication;
/// for Additional files, get_file(reference) returns a different primary file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMatch {
    pub reference: FileRef,
    pub filename: String,
    pub size: u64,
    pub hashes: Vec<FileHash>,
    pub role: MatchedFileRole,
}

impl FileMatch {
    /// Rebinds a cached platform reference to a configured source for subsequent API calls.
    pub fn for_source(&self, id: &SourceId, cache_id: &SourceId) -> Option<Self> {
        if self.reference.project.source != *cache_id {
            return None;
        }
        let mut file = self.clone();
        file.reference.project.source = id.clone();
        Some(file)
    }

    /// Converts a trusted configured-source result to its shared persistent namespace.
    pub fn for_cache(&self, id: &SourceId, cache_id: &SourceId) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.reference.project.source == *id,
            "source match belongs to another configured source"
        );
        let mut file = self.clone();
        file.reference.project.source = cache_id.clone();
        Ok(file)
    }

    /// Checks a provider match against the exact file summary used for the lookup.
    /// A matching SHA-1 digest and file size are required.
    pub fn validate(&self, summary: &super::FileSummary) -> anyhow::Result<()> {
        use anyhow::ensure;
        summary.validate()?;
        ensure!(
            !self.reference.project.source.0.is_empty()
                && !self.reference.project.id.0.is_empty()
                && !self.reference.id.0.is_empty()
                && !self.filename.is_empty(),
            "incomplete source file match"
        );
        ensure!(self.size == summary.size, "source file match size mismatch");
        let mut sha1 = false;
        for hash in &self.hashes {
            if let FileHash::Sha1(value) = hash {
                ensure!(!sha1, "duplicate source file match SHA-1");
                ensure!(
                    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid source file match SHA-1"
                );
                ensure!(
                    value.eq_ignore_ascii_case(&summary.sha1),
                    "source file match SHA-1 mismatch"
                );
                sha1 = true;
            }
        }
        ensure!(sha1, "source file match omitted SHA-1");
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedFileRole {
    Primary,
    Additional,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FileSummary, ProjectId};

    #[test]
    fn file_match_validate_checks_only_sha1_and_ignores_other_hash_formats() {
        let sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d";
        let summary = FileSummary {
            size: 3,
            sha1: sha1.into(),
            murmur2: None,
        };
        let mut matched = FileMatch {
            reference: FileRef {
                project: ProjectRef {
                    source: SourceId("modrinth".into()),
                    id: ProjectId("proj".into()),
                },
                id: FileId("ver".into()),
            },
            filename: "mod.jar".into(),
            size: 3,
            hashes: vec![
                FileHash::Md5("not-valid-md5".into()),
                FileHash::Sha512("not-valid-sha512".into()),
                FileHash::Md5("duplicate-md5".into()),
                FileHash::Sha1(sha1.to_ascii_uppercase()),
            ],
            role: MatchedFileRole::Primary,
        };
        matched.validate(&summary).unwrap();

        matched.hashes.push(FileHash::Sha1(sha1.into()));
        assert!(matched.validate(&summary).is_err());

        matched.hashes = vec![FileHash::Md5("d41d8cd98f00b204e9800998ecf8427e".into())];
        assert!(matched.validate(&summary).is_err());
    }
}
