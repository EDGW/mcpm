//! Complete Host file-browser requests, including optional Server compatibility defaults.

use super::{Configuration, build_source, server};
use anyhow::{Context, Result};
use mcpm_exec::operation::Operation;
use mcpm_lib::source::{FileId, FilePage, FileQuery, FileRef, ProjectFile, ProjectRef};
use serde::{Deserialize, Serialize};

pub struct Files;
impl Operation for Files {
    const ID: &'static str = "mod.source.files";
    type Input = Input;
    type Output = Output;
    async fn run(input: Input) -> Result<Output> {
        let mut query = input.query;
        if let Some(context) = input.server {
            query = tokio::task::spawn_blocking(move || -> Result<_> {
                context.apply(&mut query.game_versions, &mut query.loaders)?;
                Ok(query)
            })
            .await??;
        }
        let source = build_source(&input.configuration)?;
        let project = match input.project {
            Project::Identifier(value) => source.resolve_project(&value).await?,
            Project::Known(reference) => reference,
        };
        let selected = match input.selected {
            Some(id) => Some(
                source
                    .get_file(&FileRef {
                        project: project.clone(),
                        id: FileId(id.clone()),
                    })
                    .await
                    .with_context(|| format!("failed to fetch selected file {id:?}"))?,
            ),
            None => None,
        };
        let page = source.list_files(&project, &query).await?;
        Ok(Output {
            project,
            page,
            selected,
        })
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Project {
    Identifier(String),
    Known(ProjectRef),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub configuration: Configuration,
    pub project: Project,
    pub query: FileQuery,
    pub selected: Option<String>,
    pub server: Option<server::Context>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub project: ProjectRef,
    pub page: FilePage,
    pub selected: Option<ProjectFile>,
}
