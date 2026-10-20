//! Resolves whole local archive paths without requiring their metadata to be readable.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use kako_mods_lib::local::{
    mcmod::ModDirectory,
    modloader::{LoaderId, ModLoaderMeta},
};

use super::{QueryKind, Resolver, Scope, SelectionArgs};
use crate::commands::mod_support::scan::ScanSession;

/// Returns unique archive paths and the number of reported selection failures.
/// Explicit files and complete scans bypass metadata parsing; name/id queries
/// use the existing resolver, but always identify the complete outer archive.
pub(in crate::commands) fn resolve_paths(
    selection: SelectionArgs,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
) -> Result<(Vec<PathBuf>, usize)> {
    let Some(queries) = selection.into_queries() else {
        let entries = ModDirectory::new(scope.directory)?.entries()?;
        return Ok((
            entries
                .into_iter()
                .map(|entry| entry.path().to_owned())
                .collect(),
            0,
        ));
    };
    let mut resolver = Resolver::new(ScanSession::new(loaders), scope);
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut failed = 0;
    for query in queries {
        let result = (|| -> Result<Vec<PathBuf>> {
            if matches!(query.kind, QueryKind::Auto | QueryKind::File) {
                let path = resolver.scope.file_path(Path::new(&query.value))?;
                if matches!(query.kind, QueryKind::File) || path.try_exists()? {
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
            Err(error) => {
                crate::diagnostics::error(&error);
                failed += 1;
            }
        }
    }
    Ok((paths, failed + resolver.scan.failed_count()))
}
