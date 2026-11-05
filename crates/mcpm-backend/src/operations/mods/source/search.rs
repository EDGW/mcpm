//! Host project search and advisory version catalogs with optional Server defaults.

use super::{Configuration, build_source, server};
use anyhow::Result;
use mcpm_exec::operation::Operation;
use mcpm_lib::source::{SearchPage, SearchQuery};
use serde::{Deserialize, Serialize};

pub struct Search;
impl Operation for Search {
    const ID: &'static str = "mod.source.search";
    type Input = Input;
    type Output = Output;
    async fn run(input: Input) -> Result<Output> {
        match input {
            Input::Search {
                configuration,
                mut query,
                server,
            } => {
                if let Some(context) = server {
                    query = tokio::task::spawn_blocking(move || -> Result<_> {
                        context.apply(&mut query.game_versions, &mut query.loaders)?;
                        Ok(query)
                    })
                    .await??;
                }
                Ok(Output::Search(
                    build_source(&configuration)?.search(&query).await?,
                ))
            }
            Input::Versions { configuration } => Ok(Output::Versions(
                build_source(&configuration)?.game_versions().await?,
            )),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Input {
    Search {
        configuration: Configuration,
        query: SearchQuery,
        server: Option<server::Context>,
    },
    Versions {
        configuration: Configuration,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Output {
    Search(SearchPage),
    Versions(Vec<String>),
}
