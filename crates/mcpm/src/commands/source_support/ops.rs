//! Typed requests to the backend source operations.
//!
//! Commands and the TUI call these helpers; only the backend builds providers and talks to
//! a source. Every helper returns provider-neutral DTOs re-exported by the backend.

use anyhow::Result;
use mcpm_backend::source::files::{
    FilesInput, FilesOutput, FilesRequest, ProjectFiles, ProjectFilesResult,
};
use mcpm_backend::source::game::{GameVersions, GameVersionsInput};
use mcpm_backend::source::search::{Search, SearchInput};
use mcpm_backend::source::types::{FilePage, FileQuery, ProjectRef, SearchPage, SearchQuery};

use crate::commands::execution::surface_failure;

/// Runs one provider search in the calling process.
pub(in crate::commands) async fn search(
    session: &mcpm_backend::Session,
    specification: &str,
    query: SearchQuery,
) -> Result<SearchPage> {
    let mut handle = session
        .start::<Search>(SearchInput {
            specification: specification.to_owned(),
            query,
        })
        .await
        .map_err(surface_failure)?;
    while handle.next().await.is_some() {}
    handle
        .finish()
        .await
        .map_err(surface_failure)
}

/// Fetches the provider's Minecraft version catalog.
pub(in crate::commands) async fn game_versions(
    session: &mcpm_backend::Session,
    specification: &str,
) -> Result<Vec<String>> {
    let mut handle = session
        .start::<GameVersions>(GameVersionsInput {
            specification: specification.to_owned(),
        })
        .await
        .map_err(surface_failure)?;
    while handle.next().await.is_some() {}
    handle
        .finish()
        .await
        .map_err(surface_failure)
}

/// Runs one batch of project lookups or reference pages.
pub(in crate::commands) async fn files(
    session: &mcpm_backend::Session,
    specification: &str,
    requests: Vec<FilesRequest>,
) -> Result<FilesOutput> {
    let mut handle = session
        .start::<ProjectFiles>(FilesInput {
            specification: specification.to_owned(),
            requests,
        })
        .await
        .map_err(surface_failure)?;
    while handle.next().await.is_some() {}
    handle
        .finish()
        .await
        .map_err(surface_failure)
}

/// Pages an existing project reference, as incremental TUI browsing needs.
pub(in crate::commands) async fn files_page(
    session: &mcpm_backend::Session,
    specification: &str,
    project: ProjectRef,
    query: FileQuery,
) -> Result<FilePage> {
    let output = files(
        session,
        specification,
        vec![FilesRequest::Page {
            project,
            query,
            selected: None,
        }],
    )
    .await?;
    match output.results.into_iter().next() {
        Some(ProjectFilesResult::Listed { page, .. }) => Ok(*page),
        Some(ProjectFilesResult::Failed { error, .. }) => Err(error.into_error()),
        None => Err(anyhow::anyhow!("the source returned no file page")),
    }
}
