//! Ordered list query acquisition, independent of presentation and transport.

use super::super::{
    scan::{self, ScanSession},
    selection::{ResolveError, Resolver, Scope, SelectedFile, Selection},
};
use crate::diagnostics::ErrorReport;
use anyhow::Result;
use kako_mods_lib::local::modloader::{LoaderId, ModLoaderMeta};
use std::collections::HashMap;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    pub input_errors: usize,
    pub failed: usize,
}

pub(super) fn load(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    parallelism: usize,
    checkpoint: impl Fn() -> Result<()>,
    observe: impl Fn(scan::Event) -> Result<()>,
    mut emit: impl FnMut(SelectedFile) -> Result<()>,
) -> Result<Counts> {
    tracing::info!(parallelism, "Resolving server mod list selections");
    let mut resolver = Resolver::new(
        ScanSession::new(loaders)
            .with_parallelism(parallelism)
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
    };
    tracing::debug!(?counts, "Server mod list selections resolved");
    Ok(counts)
}
