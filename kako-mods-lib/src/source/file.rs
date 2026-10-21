//! Installable remote files, dependency targets, and filtered file pagination.

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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId(pub String);

/// A file publication qualified by its owning project and configured source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    pub reference: FileRef,
    pub filename: String,
    pub size: u64,
    pub hashes: Vec<FileHash>,
    pub role: MatchedFileRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedFileRole {
    Primary,
    Additional,
}
