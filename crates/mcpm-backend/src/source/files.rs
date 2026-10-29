//! `source.project.files`: resolves projects and lists their files on one provider.
//!
//! The operation stays in the calling process because it needs the provider and any API
//! key. Each request is reported independently, so one failure does not hide the others.
//! A request either resolves a source-native project value or pages an already resolved
//! reference, which is what incremental TUI browsing needs.

use anyhow::Context as _;
use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::{FileId, FilePage, FileQuery, FileRef, ProjectFile, ProjectRef};
use mcpm_exec::{Context, Operation, Placement};

use crate::env::SurfaceEnv;

/// Lists files for one or more requests of one source.
#[derive(Debug)]
pub struct ProjectFiles;

crate::surface_operation!(ProjectFiles, "source.project.files");

impl Operation for ProjectFiles {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = FilesInput;
    type Event = ();
    type Output = FilesOutput;
    type Error = FilesError;

    fn run(
        input: FilesInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            let source = cx.env().source(&input.specification).map_err(FilesError)?;
            let mut results = Vec::new();
            for request in input.requests {
                let label = request.label();
                let result = async {
                    let (project, query, selected) = match request {
                        FilesRequest::Lookup {
                            value,
                            query,
                            selected,
                        } => {
                            let project = source
                                .resolve_project(&value)
                                .await
                                .with_context(|| format!("failed to resolve project {value:?}"))?;
                            (project, query, selected)
                        }
                        FilesRequest::Page {
                            project,
                            query,
                            selected,
                        } => (project, query, selected),
                    };
                    let selected = match &selected {
                        Some(id) => Some(
                            source
                                .get_file(&FileRef {
                                    project: project.clone(),
                                    id: FileId(id.clone()),
                                })
                                .await
                                .with_context(|| {
                                    format!("failed to fetch selected file {id:?}")
                                })?,
                        ),
                        None => None,
                    };
                    let page = source
                        .list_files(&project, &query)
                        .await
                        .with_context(|| format!("failed to list files for {label:?}"))?;
                    Ok::<_, anyhow::Error>((project, page, selected))
                }
                .await;
                results.push(match result {
                    Ok((project, page, selected)) => ProjectFilesResult::Listed {
                        request: label,
                        project,
                        page: Box::new(page),
                        selected: selected.map(Box::new),
                    },
                    Err(error) => ProjectFilesResult::Failed {
                        request: label,
                        error: ErrorReport::capture(&error),
                    },
                });
            }
            Ok(FilesOutput { results })
        }
    }
}

/// One provider and the requests to serve with it.
#[derive(Debug)]
pub struct FilesInput {
    /// Source specification, possibly including a credential.
    pub specification: String,
    pub requests: Vec<FilesRequest>,
}

/// One file query: resolve a project value, or page an existing reference.
#[derive(Debug)]
pub enum FilesRequest {
    Lookup {
        value: String,
        query: FileQuery,
        selected: Option<String>,
    },
    Page {
        project: ProjectRef,
        query: FileQuery,
        selected: Option<String>,
    },
}

impl FilesRequest {
    fn label(&self) -> String {
        match self {
            Self::Lookup { value, .. } => value.clone(),
            Self::Page { project, .. } => project.id.0.clone(),
        }
    }
}

/// Per-request results in input order.
#[derive(Debug)]
pub struct FilesOutput {
    pub results: Vec<ProjectFilesResult>,
}

/// One request either resolved with its page or failed independently.
#[derive(Debug)]
pub enum ProjectFilesResult {
    Listed {
        /// The caller's project value or reference label, in request order.
        request: String,
        project: ProjectRef,
        /// Boxed because a page is much larger than the failure variant.
        page: Box<FilePage>,
        selected: Option<Box<ProjectFile>>,
    },
    Failed {
        request: String,
        error: ErrorReport,
    },
}

/// A listing could not start; per-request failures are part of the output.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct FilesError(#[from] pub anyhow::Error);
