//! Provider-neutral source data types re-exported for command inputs and presentation.
//!
//! The provider trait itself stays private to surface operations; commands only name the
//! DTOs their arguments and display already describe.

pub use kako_mc_lib::source::{
    DependencyKind, DependencyTarget, FileDependency, FileHash, FileId, FileMatch, FilePage,
    FileQuery, FileRef, FileSummary, MatchedFileRole, Project, ProjectDescription, ProjectFile,
    ProjectId, ProjectInfo, ProjectLinks, ProjectRef, ReleaseType, SearchPage, SearchQuery,
    SearchSort, SourceId, SummaryRequirements,
};
