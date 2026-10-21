//! Shared remote project identity, metadata, and search models.

use url::Url;

use super::SourceId;

/// A project summary suitable for search results and list views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub reference: ProjectRef,
    pub name: String,
    /// Short plain-text description, distinct from the full project body.
    pub summary: String,
    pub slug: Option<String>,
    pub icon_url: Option<Url>,
    pub website_url: Option<Url>,
}

/// Project details, independent of release and downloadable file metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub info: ProjectInfo,
    /// `None` means the source does not provide a project body.
    pub description: Option<ProjectDescription>,
    pub links: ProjectLinks,
}

/// A project body retaining its original format for presentation by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectDescription {
    Markdown(String),
    Html(String),
    PlainText(String),
}

/// Optional external destinations associated with a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectLinks {
    pub source_code: Option<Url>,
    pub issues: Option<Url>,
    pub wiki: Option<Url>,
    pub discord: Option<Url>,
}

/// A project identity qualified by the configured source that can resolve it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectRef {
    pub source: SourceId,
    pub id: ProjectId,
}

/// An opaque, case-sensitive project identifier in a source's namespace.
///
/// Sources normalize their native identifiers into strings. This is a platform
/// project identifier, not a mod loader's mod ID, a slug, or a release ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(pub String);

/// Common search criteria for remote mod projects.
///
/// Fields are combined with AND; alternatives within each list use OR.
/// Version and loader strings are
/// interpreted by the source; an unfiltered search does not establish that a
/// particular downloadable release is compatible with the caller's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub query: String,
    /// Match any listed Minecraft version; empty means unrestricted.
    pub game_versions: Vec<String>,
    /// Match any listed loader; empty means unrestricted.
    pub loaders: Vec<String>,
    pub sort: SearchSort,
    /// Number of matching projects to skip, not a page number.
    pub offset: u64,
    /// Maximum number of projects to return; must be greater than zero.
    pub limit: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            game_versions: Vec::new(),
            loaders: Vec::new(),
            sort: SearchSort::default(),
            offset: 0,
            limit: 20,
        }
    }
}

/// Search ordering, subject to support by the selected source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchSort {
    /// Use the source's default ordering without assuming a relevance algorithm.
    #[default]
    SourceDefault,
    /// Highest total project download count first.
    Downloads,
    /// Most recently updated projects first.
    Updated,
    /// Most recently created projects first.
    Newest,
}

/// One page of matching projects with optional total-count metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    pub projects: Vec<ProjectInfo>,
    /// Effective offset used for this page.
    pub offset: u64,
    /// `None` means the source did not report a total, rather than zero matches.
    pub total: Option<u64>,
}
