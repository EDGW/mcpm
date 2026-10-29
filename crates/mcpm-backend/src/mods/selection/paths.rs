//! Resolves whole archive paths without requiring their metadata to be readable.
//!
//! Explicit files and full scans bypass metadata parsing; name/id queries use the shared
//! resolver with caller-owned cancellation and scan concurrency.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::local::modloader::{LoaderId, ModLoaderMeta};

use super::{FileScope, Query, QueryKind, ResolveError, Resolver, Selection};
use crate::data::runtime::RuntimeEnvironment;
use crate::mods::scan::{Event, ScanSession};

/// Returns unique outer archive paths and the number of diagnosed selection failures.
pub fn resolve_paths(
    selection: Selection,
    scope: FileScope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    environment: &RuntimeEnvironment,
    parallelism: usize,
    checkpoint: impl Fn() -> Result<()>,
    observe: impl Fn(Event) -> Result<()>,
) -> Result<(Vec<PathBuf>, usize)> {
    checkpoint()?;
    let queries = match selection.into_queries() {
        Some(queries) => queries,
        None => {
            let mut resolver = Resolver::new(
                ScanSession::new(loaders, environment)
                    .with_parallelism(parallelism)
                    .with_checkpoint(&checkpoint)
                    .with_observer(&observe),
                scope,
            );
            let files = resolver.all().map_err(execution)?;
            let paths = files
                .into_iter()
                .map(|selected| selected.scanned.file.entry.path().to_owned())
                .collect();
            return Ok((paths, resolver.scan.failed_count()));
        }
    };
    let mut resolver = Resolver::new(
        ScanSession::new(loaders, environment)
            .with_parallelism(parallelism)
            .with_checkpoint(&checkpoint)
            .with_observer(&observe),
        scope,
    );
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut failed = 0;
    for query in queries {
        checkpoint()?;
        match explicit_path(&query, &resolver) {
            Ok(Some(path)) => {
                if seen.insert(path.clone()) {
                    paths.push(path);
                }
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                observe(Event::Diagnostic {
                    error: ErrorReport::capture(&error),
                })?;
                failed += 1;
                continue;
            }
        }
        let result = match query.kind {
            QueryKind::Auto | QueryKind::File => resolver.resolve_list(query),
            QueryKind::ModId | QueryKind::Name => resolver.resolve(query),
        };
        match result {
            Ok(selected) => {
                for selected in selected {
                    let path = selected.scanned.file.entry.path().to_owned();
                    if seen.insert(path.clone()) {
                        paths.push(path);
                    }
                }
            }
            Err(ResolveError::Execution(error)) => return Err(error),
            Err(ResolveError::Input(error)) => {
                observe(Event::Diagnostic {
                    error: ErrorReport::capture(&error),
                })?;
                failed += 1;
            }
        }
    }
    Ok((paths, failed + resolver.scan.failed_count()))
}

/// Explicit files bypass inspection so a later import can report its own failure.
fn explicit_path(query: &Query, resolver: &Resolver<'_>) -> Result<Option<PathBuf>> {
    match query.kind {
        QueryKind::File => {
            let path = resolver.file_path(std::path::Path::new(&query.value))?;
            Ok(Some(path))
        }
        QueryKind::Auto => {
            let path = resolver.file_path(std::path::Path::new(&query.value))?;
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => Ok(Some(path)),
                Ok(_) => Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(anyhow::Error::new(error)
                    .context(format!("failed to inspect {path:?}"))),
            }
        }
        QueryKind::ModId | QueryKind::Name => Ok(None),
    }
}

fn execution(error: ResolveError) -> anyhow::Error {
    match error {
        ResolveError::Input(error) | ResolveError::Execution(error) => error,
    }
}
