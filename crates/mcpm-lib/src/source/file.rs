//! Installable remote files, dependency targets, and filtered file pagination.

use serde::{Deserialize, Serialize};
use url::Url;

use super::ProjectRef;

/// An installable file identified by a publication in its project.
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
}

/// Hex-encoded checksums of the downloadable file, not provider fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algorithm", content = "value", rename_all = "snake_case")]
pub enum FileHash {
    Sha1(String),
    Sha512(String),
    Md5(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDependency {
    pub target: DependencyTarget,
    pub kind: DependencyKind,
}

/// Dependency declarations do not imply a version range or proven compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyTarget {
    Project(ProjectRef),
    File(FileRef),
    /// A filename-only declaration that cannot be resolved to a project.
    Filename(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
    Tool,
    Include,
}

/// AND-combined fields, OR within each list, applied before pagination; newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn for_source(&self, source: &dyn super::Source) -> Option<Self> {
        if self.reference.project.source != source.identification_cache_id() {
            return None;
        }
        let mut file = self.clone();
        file.reference.project.source = source.id().clone();
        Some(file)
    }

    /// Converts a trusted configured-source result to its shared persistent namespace.
    pub fn for_cache(&self, source: &dyn super::Source) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.reference.project.source == *source.id(),
            "source match belongs to another configured source"
        );
        let mut file = self.clone();
        file.reference.project.source = source.identification_cache_id();
        Ok(file)
    }

    /// Checks a provider match against the exact file summary used for the lookup.
    /// A SHA-1 match and size are required; all supplied digest syntax is checked.
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
        let mut seen = std::collections::HashSet::new();
        for hash in &self.hashes {
            let (algorithm, value, length) = match hash {
                FileHash::Sha1(value) => ("sha1", value, 40),
                FileHash::Sha512(value) => ("sha512", value, 128),
                FileHash::Md5(value) => ("md5", value, 32),
            };
            ensure!(seen.insert(algorithm), "duplicate source file match hash");
            ensure!(
                value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid source file match hash"
            );
            if algorithm == "sha1" {
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
