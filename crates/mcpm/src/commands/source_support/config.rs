//! Typed Host source requests shared by command output and interactive browsers.

use anyhow::{Result, bail};
use mcpm_backend::operations::mods::source::{self, Configuration};
use mcpm_exec::service::Executor;
use mcpm_lib::source::{FileQuery, ProjectRef, SearchPage, SearchQuery};

pub(in crate::commands) async fn search(
    executor: &Executor,
    configuration: &Configuration,
    query: &SearchQuery,
    server: Option<&source::server::Context>,
) -> Result<SearchPage> {
    match executor
        .call::<source::search::Search>(source::search::Input::Search {
            configuration: configuration.clone(),
            query: query.clone(),
            server: server.cloned(),
        })
        .await?
    {
        source::search::Output::Search(page) => Ok(page),
        _ => bail!("unexpected source search response"),
    }
}
pub(in crate::commands) async fn versions(
    executor: &Executor,
    configuration: &Configuration,
) -> Result<Vec<String>> {
    match executor
        .call::<source::search::Search>(source::search::Input::Versions {
            configuration: configuration.clone(),
        })
        .await?
    {
        source::search::Output::Versions(versions) => Ok(versions),
        _ => bail!("unexpected source version response"),
    }
}
pub(in crate::commands) async fn files(
    executor: &Executor,
    configuration: &Configuration,
    project: source::files::Project,
    query: &FileQuery,
    selected: Option<String>,
    server: Option<&source::server::Context>,
) -> Result<source::files::Output> {
    executor
        .call::<source::files::Files>(source::files::Input {
            configuration: configuration.clone(),
            project,
            query: query.clone(),
            selected,
            server: server.cloned(),
        })
        .await
}
pub(in crate::commands) async fn file_page(
    executor: &Executor,
    configuration: &Configuration,
    project: &ProjectRef,
    query: &FileQuery,
    server: Option<&source::server::Context>,
) -> Result<mcpm_lib::source::FilePage> {
    Ok(files(
        executor,
        configuration,
        source::files::Project::Known(project.clone()),
        query,
        None,
        server,
    )
    .await?
    .page)
}
