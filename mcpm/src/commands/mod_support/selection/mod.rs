//! Query resolution, lazy directory indexes and selected declaration identities.

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use thiserror::Error;

use super::{
    mod_records::{self, RecordKey},
    scan::{ScanSession, ScannedFile},
};

mod args;
pub(in crate::commands) use args::{SelectionArgs, StdinArgs};
mod paths;
pub(in crate::commands) use paths::resolve_paths;

#[derive(Clone, Copy)]
pub(super) enum QueryKind {
    Auto,
    File,
    ModId,
    Name,
}

pub(super) struct Query {
    kind: QueryKind,
    value: OsString,
}

/// File inputs either use ordinary paths or paths scoped to a server mod directory.
pub(in crate::commands) struct Scope {
    directory: PathBuf,
    container_paths: bool,
}

impl Scope {
    pub(in crate::commands) fn directory(directory: PathBuf) -> Self {
        Self {
            directory,
            container_paths: false,
        }
    }

    pub(in crate::commands) fn container(directory: PathBuf) -> Self {
        Self {
            directory,
            container_paths: true,
        }
    }

    pub(in crate::commands) fn file_path(&self, value: &Path) -> Result<PathBuf> {
        if !self.container_paths {
            return Ok(if self.directory == Path::new(".") {
                value.to_owned()
            } else {
                self.directory.join(value)
            });
        }
        if value.as_os_str().is_empty()
            || value
                .components()
                .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err(SelectionError::InvalidModPath {
                path: value.to_owned(),
            }
            .into());
        }
        let relative: PathBuf = value
            .components()
            .filter(|part| !matches!(part, Component::CurDir))
            .collect();
        Ok(self
            .directory
            .join(relative.strip_prefix("mods").unwrap_or(&relative)))
    }
}

pub(super) struct SelectedFile {
    pub scanned: Arc<ScannedFile>,
    /// None selects the whole file; Some selects the owning archives of these declarations.
    pub records: Option<Vec<RecordKey>>,
}

type Index = HashMap<String, Vec<(Arc<ScannedFile>, RecordKey)>>;

/// One query session owns its cache and initializes name/id indexes only on demand.
pub(super) struct Resolver {
    pub scan: ScanSession,
    scope: Scope,
    directory: Option<Vec<Arc<ScannedFile>>>,
    ids: Index,
    names: Index,
}

impl Resolver {
    pub(super) fn new(scan: ScanSession, scope: Scope) -> Self {
        Self {
            scan,
            scope,
            directory: None,
            ids: HashMap::new(),
            names: HashMap::new(),
        }
    }

    pub(super) fn all(&mut self) -> Result<Vec<SelectedFile>> {
        self.ensure_index()?;
        Ok(self
            .directory
            .as_ref()
            .unwrap()
            .iter()
            .cloned()
            .map(|scanned| SelectedFile {
                scanned,
                records: None,
            })
            .collect())
    }

    pub(super) fn resolve(&mut self, query: Query) -> Result<Vec<SelectedFile>> {
        self.resolve_query(query, false)
    }

    /// Lists also accept directories, while info/deps remain file inspections.
    pub(super) fn resolve_list(&mut self, query: Query) -> Result<Vec<SelectedFile>> {
        self.resolve_query(query, true)
    }

    fn resolve_query(&mut self, query: Query, directories: bool) -> Result<Vec<SelectedFile>> {
        if matches!(query.kind, QueryKind::Auto | QueryKind::File) {
            let path = self.scope.file_path(Path::new(&query.value))?;
            let found = if matches!(query.kind, QueryKind::File) {
                true
            } else {
                match std::fs::metadata(&path) {
                    Ok(_) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(error).with_context(|| format!("failed to inspect {path:?}"));
                    }
                }
            };
            if found && directories && path.is_dir() {
                return Ok(self
                    .scan
                    .directory(&path)?
                    .into_iter()
                    .map(|scanned| SelectedFile {
                        scanned,
                        records: None,
                    })
                    .collect());
            }
            if found {
                return Ok(self
                    .scan
                    .read(path, None)
                    .into_iter()
                    .map(|scanned| SelectedFile {
                        scanned,
                        records: None,
                    })
                    .collect());
            }
        }
        self.ensure_index()?;
        let value = query.value.to_str().ok_or(SelectionError::NonUtf8Query)?;
        let matches = match query.kind {
            QueryKind::Auto => self.ids.get(value).or_else(|| self.names.get(value)),
            QueryKind::ModId => self.ids.get(value),
            QueryKind::Name => self.names.get(value),
            QueryKind::File => unreachable!(),
        }
        .ok_or_else(|| SelectionError::NotFound {
            value: query.value.clone(),
        })?;
        let mut selected: Vec<SelectedFile> = Vec::new();
        for (file, key) in matches {
            if let Some(previous) = selected
                .iter_mut()
                .find(|previous| Arc::ptr_eq(&previous.scanned, file))
            {
                let records = previous.records.as_mut().unwrap();
                if !records.contains(key) {
                    records.push(*key);
                }
            } else {
                selected.push(SelectedFile {
                    scanned: Arc::clone(file),
                    records: Some(vec![*key]),
                });
            }
        }
        Ok(selected)
    }

    fn ensure_index(&mut self) -> Result<()> {
        if self.directory.is_some() {
            return Ok(());
        }
        let files = self.scan.directory(&self.scope.directory)?;
        for scanned in &files {
            for (metadata_index, metadata) in scanned.file.metadata.iter().enumerate() {
                for (record_index, record) in mod_records::collect(metadata, "").iter().enumerate()
                {
                    let key = RecordKey {
                        metadata: metadata_index,
                        record: record_index,
                    };
                    self.ids
                        .entry(record.info.modid.clone())
                        .or_default()
                        .push((Arc::clone(scanned), key));
                    self.names
                        .entry(record.info.name.clone())
                        .or_default()
                        .push((Arc::clone(scanned), key));
                }
            }
        }
        self.directory = Some(files);
        Ok(())
    }
}

#[derive(Debug, Error)]
enum SelectionError {
    #[error("no mod matched input {value:?}")]
    NotFound { value: OsString },
    #[error("modid and mod name queries must be valid UTF-8")]
    NonUtf8Query,
    #[error(
        "mod file {path:?} must be relative to the server's mods directory without parent traversal"
    )]
    InvalidModPath { path: PathBuf },
}
