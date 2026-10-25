//! Ordered list query acquisition, independent of presentation and transport.

use super::super::{
    scan::{self, ScanSession},
    selection::{ResolveError, Resolver, Scope, SelectedFile, Selection},
};
use crate::diagnostics::ErrorReport;
use anyhow::Result;
use mcpm_backend::local::modloader::{LoaderId, ModLoaderMeta};
use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::collections::HashMap;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    pub input_errors: usize,
    pub failed: usize,
    pub cache: super::super::scan::CacheSummary,
}

pub(super) fn load(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    environment: &RuntimeEnvironment,
    loading: impl Into<scan::Options>,
    checkpoint: impl Fn() -> Result<()>,
    observe: impl Fn(scan::Event) -> Result<()>,
    mut emit: impl FnMut(SelectedFile) -> Result<()>,
) -> Result<Counts> {
    tracing::info!("Resolving server mod list selections");
    let mut resolver = Resolver::new(
        ScanSession::new(loaders, environment)
            .with_options(loading.into())?
            .with_checkpoint(&checkpoint)
            .with_observer(&observe),
        scope,
    );
    let mut input_errors = 0;
    let queries = match selection.into_queries() {
        Some(queries) if !queries.is_empty() => queries.into_iter().map(Some).collect(),
        _ => vec![None],
    };
    for query in queries {
        checkpoint()?;
        let result = match query {
            Some(query) => resolver.resolve_list(query),
            None => resolver.all(),
        };
        match result {
            Ok(selected) => {
                for file in selected {
                    checkpoint()?;
                    emit(file)?;
                }
            }
            Err(ResolveError::Execution(error)) => return Err(error),
            Err(ResolveError::Input(error)) => {
                observe(scan::Event::Diagnostic {
                    error: ErrorReport::capture(&error),
                })?;
                input_errors += 1;
            }
        }
    }
    let counts = Counts {
        input_errors,
        failed: resolver.scan.failed_count(),
        cache: resolver.scan.cache_summary(),
    };
    tracing::debug!(?counts, "Server mod list selections resolved");
    Ok(counts)
}
