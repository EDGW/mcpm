//! Operation-local lazy providers; all verified hits are collected before any miss is requested.

use super::{PreparedIdentification, QueryResult, SourceResolution, SourceResult};
use crate::operations::mods::source::{Configuration, Description, build_source};
use anyhow::{Context, Result, ensure};
use mcpm_exec::error::PortableError;
use mcpm_lib::source::{Source, SourceId, SummaryRequirements};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::OnceCell;

type Client = Result<Box<dyn Source>, Arc<PortableError>>;

pub(crate) struct Sources {
    sources: Vec<PlannedSource>,
    cache_ids: Vec<String>,
    requirements: SummaryRequirements,
}
impl Sources {
    pub(crate) fn new(configurations: Vec<Configuration>) -> Result<Self> {
        ensure!(
            !configurations.is_empty(),
            "select at least one identification source"
        );
        let mut known = HashSet::new();
        let mut sources = Vec::with_capacity(configurations.len());
        for configuration in configurations {
            let description = configuration
                .describe()
                .map_err(|error| Arc::new(PortableError::from(error)));
            if let Ok(description) = &description {
                ensure!(
                    known.insert(description.cache_id.0.clone()),
                    "select each identification source only once"
                );
            }
            sources.push(PlannedSource {
                configuration,
                description,
                client: OnceCell::new(),
            });
        }
        let cache_ids = sources
            .iter()
            .filter_map(|source| source.description.as_ref().ok())
            .map(|source| source.cache_id.0.clone())
            .collect();
        let requirements = SummaryRequirements {
            murmur2: sources
                .iter()
                .filter_map(|source| source.description.as_ref().ok())
                .any(|source| source.requirements.murmur2),
        };
        Ok(Self {
            sources,
            cache_ids,
            requirements,
        })
    }
    pub(crate) fn requirements(&self) -> SummaryRequirements {
        self.requirements
    }
    pub(crate) fn cache_ids(&self) -> &[String] {
        &self.cache_ids
    }
    /// Refresh can stop source requests on the first failure without discarding later cache hits.
    pub(crate) async fn identify(
        &self,
        prepared: &PreparedIdentification,
        stop_on_error: bool,
    ) -> QueryResult {
        let mut results: Vec<_> = self
            .sources
            .iter()
            .map(|source| {
                let (id, cache_id, cached) = match &source.description {
                    Ok(description) => (
                        description.id.clone(),
                        Some(description.cache_id.clone()),
                        prepared
                            .cached
                            .get(&description.cache_id.0)
                            .and_then(|file| {
                                file.for_source(&description.id, &description.cache_id)
                            }),
                    ),
                    Err(_) => (
                        SourceId(source.configuration.label().to_owned()),
                        None,
                        None,
                    ),
                };
                SourceResult {
                    source: id,
                    cache_id,
                    resolution: cached.map_or(SourceResolution::Skipped, SourceResolution::Cached),
                }
            })
            .collect();
        for (source, result) in self.sources.iter().zip(&mut results) {
            if matches!(result.resolution, SourceResolution::Cached(_)) {
                continue;
            }
            let outcome = match &source.description {
                Err(error) => Err(error.clone()),
                Ok(description) => {
                    let client = source
                        .client
                        .get_or_init(|| async {
                            build_source(&source.configuration)
                                .map_err(|error| Arc::new(PortableError::from(error)))
                        })
                        .await;
                    match client {
                        Err(error) => Err(error.clone()),
                        Ok(client) => client
                            .identify_summary(&prepared.summary)
                            .await
                            .with_context(|| {
                                format!("source identification failed for {}", description.id.0)
                            })
                            .map_err(|error| Arc::new(PortableError::from(error))),
                    }
                }
            };
            let failed = outcome.is_err();
            result.resolution = match outcome {
                Ok(file) => SourceResolution::Queried(file),
                Err(error) => SourceResolution::Failed(error),
            };
            if failed && stop_on_error {
                break;
            }
        }
        QueryResult { sources: results }
    }
}

struct PlannedSource {
    configuration: Configuration,
    description: Result<Description, Arc<PortableError>>,
    client: OnceCell<Client>,
}
