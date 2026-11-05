//! Provider-independent source operations and configured source identities.

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    FileMatch, FilePage, FileQuery, FileRef, FileSummary, Project, ProjectFile, ProjectRef,
    SearchPage, SearchQuery, SummaryRequirements,
};

/// A remote mod source with its own identifiers.
///
/// Implementations normalize provider responses without exposing transport DTOs.
/// Returned futures are `Send` and run on Tokio, the project async runtime.
/// File identification uses its blocking pool for synchronous summary computation.
/// Supports dynamic dispatch through `Box<dyn Source>`. Implementations must
/// also use `#[async_trait::async_trait]`.
#[async_trait]
pub trait Source: Send + Sync {
    /// Returns the stable identity of this configured source.
    fn id(&self) -> &SourceId;

    /// Namespace for verified content matches. Credentials and transport endpoints
    /// must not be included. Providers may share this across configurations.
    fn identification_cache_id(&self) -> SourceId {
        self.id().clone()
    }

    /// Searches for projects and returns one page of summaries.
    ///
    /// Returned references must belong to this source. Implementations must
    /// report unsupported filters or sorting as errors rather
    /// than silently ignoring them. `limit` must be greater than zero.
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage>;

    /// Fetches the source's Minecraft version catalog in provider order.
    ///
    /// Values are unique, nonempty search filter identifiers. This catalog is
    /// advisory: a version absent from it must not be rejected locally. Callers
    /// control caching and refresh policy; fetching does not search for projects.
    async fn game_versions(&self) -> Result<Vec<String>>;

    /// Fetches project details, performing multiple requests if necessary.
    ///
    /// The implementation must reject references whose source differs from
    /// `self.id()`. A missing project is an implementation-specific error.
    async fn get_project(&self, project: &ProjectRef) -> Result<Project>;

    /// Resolves a source-native project ID or exact slug to a canonical reference.
    async fn resolve_project(&self, id_or_slug: &str) -> Result<ProjectRef>;

    /// Lists installable files in descending publication time, filtering before
    /// pagination. The project must belong to this source. Unsupported filters
    /// and invalid pagination are errors, never silently ignored.
    async fn list_files(&self, project: &ProjectRef, query: &FileQuery) -> Result<FilePage>;

    /// Fetches one installable file. Both source and project ownership must be
    /// checked. Modrinth references identify a Version's sole primary file.
    async fn get_file(&self, file: &FileRef) -> Result<ProjectFile>;

    /// Identifies the bytes of a local file without modifying it. Returns None
    /// when no unique verified match exists, including ambiguous matches.
    /// I/O, transport, and malformed responses are errors.
    /// The caller must keep the file unchanged during lookup.
    /// Additional Modrinth files retain their actual metadata, not the primary's.
    async fn identify_file(&self, path: &Path) -> Result<Option<FileMatch>> {
        let path = path.to_owned();
        let requirements = self.summary_requirements();
        let summary =
            tokio::task::spawn_blocking(move || FileSummary::read(&path, requirements)).await??;
        self.identify_summary(&summary).await
    }

    /// Declare the digest algorithms required for content identification.
    fn summary_requirements(&self) -> SummaryRequirements {
        SummaryRequirements::default()
    }

    /// Query the provider using already-computed digests, without accessing local files.
    /// Implementations must verify returned size and hashes before accepting a match.
    /// Returns None if there is no unique verified match, including ambiguous results.
    async fn identify_summary(&self, summary: &FileSummary) -> Result<Option<FileMatch>>;
}

/// An open, case-sensitive identifier for a configured source.
///
/// Callers assign distinct identifiers to source configurations that must be
/// addressed separately. No built-in provider registry is assumed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub String);
