//! Provider-independent source operations and configured source identities.

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use super::{
    FileMatch, FilePage, FileQuery, FileRef, Project, ProjectFile, ProjectRef, SearchPage,
    SearchQuery,
};

/// A remote mod source with its own identifiers.
///
/// Implementations normalize provider responses without exposing transport DTOs.
/// Returned futures are `Send`. The interface does not prescribe an async
/// runtime; concrete implementations may have their own runtime requirements.
/// Supports dynamic dispatch through `Box<dyn Source>`. Implementations must
/// also use `#[async_trait::async_trait]`.
#[async_trait]
pub trait Source: Send + Sync {
    /// Returns the stable identity of this configured source.
    fn id(&self) -> &SourceId;

    /// Searches for projects and returns one page of summaries.
    ///
    /// Returned references must belong to this source. Implementations must
    /// report unsupported filters or sorting as errors rather
    /// than silently ignoring them. `limit` must be greater than zero.
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage>;

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
    /// for no verified match; I/O, transport, malformed responses, and ambiguous
    /// matches are errors. The caller must keep the file unchanged during lookup.
    /// Additional Modrinth files retain their actual metadata, not the primary's.
    async fn identify_file(&self, path: &Path) -> Result<Option<FileMatch>>;
}

/// An open, case-sensitive identifier for a configured source.
///
/// Callers assign distinct identifiers to source configurations that must be
/// addressed separately. No built-in provider registry is assumed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceId(pub String);
