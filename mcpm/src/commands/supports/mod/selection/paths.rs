//! Resolves whole local archive paths without requiring their metadata to be readable.

use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use mcpm_backend::local::{
    mcmod::ModDirectory,
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{QueryKind, ResolveError, Resolver, Scope, Selection};
use crate::{
    commands::supports::mods::scan::{Event, ScanSession},
    diagnostics::ErrorReport,
};

/// Returns unique outer archive paths and the number of diagnosed selection failures.
/// Explicit files and full scans bypass metadata parsing; name/id queries use the
/// shared resolver with caller-owned cancellation and scan concurrency.
pub(in crate::commands) fn resolve_paths(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    environment: &RuntimeEnvironment,
    parallelism: usize,
    checkpoint: impl Fn() -> Result<()>,
    observe: impl Fn(Event) -> Result<()>,
) -> Result<(Vec<PathBuf>, usize)> {
    checkpoint()?;
    let Some(queries) = selection.into_queries() else {
        let entries = ModDirectory::new(scope.directory)?.entries()?;
        checkpoint()?;
        return Ok((
            entries
                .into_iter()
                .map(|entry| entry.path().to_owned())
                .collect(),
            0,
        ));
    };
    let mut resolver = Resolver::new(
        ScanSession::new(loaders, environment)
            .with_parallelism(parallelism)
            .with_checkpoint(&checkpoint)
            .with_observer(&observe),
        scope,
    );
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut failed = 0;
    for query in queries {
        checkpoint()?;
        let result = (|| -> std::result::Result<Vec<PathBuf>, ResolveError> {
            if matches!(query.kind, QueryKind::Auto | QueryKind::File) {
                let path = resolver
                    .scope
                    .file_path(Path::new(&query.value))
                    .map_err(ResolveError::Input)?;
                if matches!(query.kind, QueryKind::File)
                    || path
                        .try_exists()
                        .map_err(|error| ResolveError::Input(error.into()))?
                {
                    return Ok(vec![path]);
                }
            }
            Ok(resolver
                .resolve(query)?
                .into_iter()
                .map(|selected| selected.scanned.file.entry.path().to_owned())
                .collect())
        })();
        match result {
            Ok(selected) => {
                for path in selected {
                    let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    if seen.insert(key) {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::commands::supports::mods::selection::SelectionInput;

    fn modid_selection() -> Selection {
        serde_json::from_value::<SelectionInput>(serde_json::json!({
            "queries": [{"kind": "modid", "value": "missing"}]
        }))
        .unwrap()
        .into_selection()
        .unwrap()
    }

    #[test]
    fn cancellation_during_lazy_index_scan_remains_fatal() {
        let directory = tempfile::tempdir().unwrap();
        let cancelled = Cell::new(false);
        let diagnostics = Cell::new(0);
        let error = resolve_paths(
            modid_selection(),
            Scope::container(directory.path().to_owned()),
            HashMap::new(),
            &RuntimeEnvironment::new().with_mods_cache(None),
            2,
            || {
                anyhow::ensure!(!cancelled.get(), "selection cancelled");
                Ok(())
            },
            |event| {
                match event {
                    Event::Progress { .. } => cancelled.set(true),
                    Event::Diagnostic { .. } => diagnostics.set(diagnostics.get() + 1),
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "selection cancelled");
        assert_eq!(diagnostics.get(), 0);
    }

    #[test]
    fn consumer_failure_during_lazy_index_scan_is_not_a_selection_failure() {
        let directory = tempfile::tempdir().unwrap();
        let events = Cell::new(0);
        let error = resolve_paths(
            modid_selection(),
            Scope::container(directory.path().to_owned()),
            HashMap::new(),
            &RuntimeEnvironment::new().with_mods_cache(None),
            2,
            || Ok(()),
            |_| {
                events.set(events.get() + 1);
                anyhow::bail!("transport closed")
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "transport closed");
        assert_eq!(events.get(), 1);
    }
}
