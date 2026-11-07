//! Complete Host identification: read and verify files, reuse each source hit, query only misses.

use super::Configuration;
use crate::operations::mods::{
    identification::{self, QueryResult, QuerySummary, Sources},
    loading::{
        self, ScopeInput,
        selection::{SelectionInput, resolve_paths},
    },
};
use anyhow::Result;
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::{cache::mods::ModsCacheSelection, runtime::RuntimeEnvironment};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub struct Identify;
impl Operation for Identify {
    const ID: &'static str = "mod.source.identify";
    type Input = Input;
    type Output = StreamingOutput<Event>;
    async fn run(input: Input) -> Result<Self::Output> {
        let sources = Arc::new(Sources::new(input.sources)?);
        let parallelism = input.parallelism.max(1);
        let target = tokio::task::spawn_blocking(move || -> Result<_> {
            match input.target {
                Target::Paths { paths, cache } => {
                    let environment = RuntimeEnvironment::new().with_mods_cache(cache);
                    environment.mods_cache()?;
                    Ok(ReadyTarget::Paths { paths, environment })
                }
                Target::Selection { scope, selection } => {
                    let resolved = scope.resolve()?;
                    resolved.environment.mods_cache()?;
                    Ok(ReadyTarget::Selection {
                        resolved,
                        selection: selection.into_selection()?,
                    })
                }
            }
        })
        .await??;
        Ok(StreamingOutput::new(move |sender| async move {
            let checkpoint = sender.clone();
            let (paths, environment, selection_failed, diagnostics) =
                tokio::task::spawn_blocking(move || -> Result<_> {
                    match target {
                        ReadyTarget::Paths { paths, environment } => {
                            Ok((paths, environment, 0, Vec::new()))
                        }
                        ReadyTarget::Selection {
                            resolved,
                            selection,
                        } => {
                            let diagnostics = std::cell::RefCell::new(Vec::new());
                            let (paths, failed) = resolve_paths(
                                selection,
                                resolved.scope,
                                resolved.loaders,
                                &resolved.environment,
                                parallelism,
                                || checkpoint.checkpoint().map_err(Into::into),
                                |event| {
                                    if let loading::scan::Event::Diagnostic { error } = event {
                                        diagnostics.borrow_mut().push(error);
                                    }
                                    Ok(())
                                },
                            )?;
                            Ok((
                                paths,
                                resolved.environment,
                                failed,
                                diagnostics.into_inner(),
                            ))
                        }
                    }
                })
                .await??;
            let grouping = sender.clone();
            let (paths, jobs) = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut positions = HashMap::new();
                let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
                for (index, path) in paths.iter().enumerate() {
                    grouping.checkpoint()?;
                    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                    let position = *positions.entry(key).or_insert_with(|| {
                        jobs.push((path.clone(), Vec::new()));
                        jobs.len() - 1
                    });
                    jobs[position].1.push(index);
                }
                Ok((paths, jobs))
            })
            .await??;
            sender
                .send(Event::Prepared {
                    paths,
                    selection_failed,
                    diagnostics,
                })
                .await?;
            let mut summary = QuerySummary {
                total: jobs.len(),
                selection_failed,
                ..Default::default()
            };
            let environment = Arc::new(environment);
            let requirements = sources.requirements();
            let mut pending = jobs.into_iter();
            let mut active = tokio::task::JoinSet::new();
            loop {
                while active.len() < parallelism {
                    let Some((path, indices)) = pending.next() else {
                        break;
                    };
                    sender.checkpoint()?;
                    sender
                        .send(Event::Loading {
                            indices: indices.clone(),
                        })
                        .await?;
                    let environment = environment.clone();
                    let preparing_sources = sources.clone();
                    let sources = sources.clone();
                    active.spawn(async move {
                        let prepared = tokio::task::spawn_blocking(move || {
                            identification::prepare_environment(
                                &path,
                                requirements,
                                preparing_sources.cache_ids(),
                                &environment,
                                input.force,
                            )
                        })
                        .await;
                        let result = match prepared {
                            Ok(Ok(prepared)) => Ok(sources.identify(&prepared, false).await),
                            Ok(Err(error)) => Err(PortableError::from(error)),
                            Err(error) => Err(PortableError::from(anyhow::Error::from(error))),
                        };
                        (indices, result)
                    });
                }
                let Some(completed) = active.join_next().await else {
                    break;
                };
                let (indices, result) = completed?;
                match &result {
                    Ok(result) => summary.record(result),
                    Err(_) => summary.preparation_failed += 1,
                }
                sender.send(Event::Item { indices, result }).await?;
            }
            sender.send(Event::Done(summary)).await?;
            Ok(())
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Target {
    Paths {
        paths: Vec<PathBuf>,
        cache: Option<ModsCacheSelection>,
    },
    Selection {
        scope: ScopeInput,
        selection: SelectionInput,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub target: Target,
    pub sources: Vec<Configuration>,
    pub force: bool,
    pub parallelism: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Prepared {
        paths: Vec<PathBuf>,
        selection_failed: usize,
        diagnostics: Vec<PortableError>,
    },
    Loading {
        indices: Vec<usize>,
    },
    Item {
        indices: Vec<usize>,
        result: Result<QueryResult, PortableError>,
    },
    Done(QuerySummary),
}

enum ReadyTarget {
    Paths {
        paths: Vec<PathBuf>,
        environment: RuntimeEnvironment,
    },
    Selection {
        resolved: loading::ResolvedScope,
        selection: loading::selection::Selection,
    },
}
