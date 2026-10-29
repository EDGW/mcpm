//! JSON-visible list records shared by path-based and server-scoped listing.

use std::{path::Path, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, ensure};
use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::local::mcmod::{
    CachedModMeta, MetadataConversionError, ModEntry, ModEntryName, ModFile, ModMeta, ModStatus,
};
use kako_mc_lib::local::modloader::{BUILTIN_LOADERS, ModLoaderMeta};
use serde::{Deserialize, Serialize};

use super::declarations::{self, RecordKey};
use crate::mods::scan::{CacheSummary, FailedIdentification, ScanResult, ScannedFile};
use crate::mods::selection::SelectedFile;

/// One outer file, or an input that could not be read.
#[derive(Serialize, Deserialize, Debug)]
pub struct Record {
    pub path: String,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub status: Status,
    pub metadata: Vec<CachedModMeta>,
    pub errors: Vec<LoadError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<RecordKey>>,
}

impl Record {
    pub(crate) fn from_scan(
        path: &Path,
        result: &ScanResult,
    ) -> Result<Self, MetadataConversionError> {
        let file = result.file.as_ref().map(|scanned| &scanned.file);
        Ok(Self {
            records: None,
            path: path.to_string_lossy().into_owned(),
            name: file.map(|file| file.entry.name().to_owned()),
            enabled: file.map(|file| matches!(file.entry.status(), ModStatus::Enabled)),
            status: if result.failed() {
                Status::Failed
            } else if file.is_some_and(|file| file.metadata.iter().any(ModMeta::has_mods)) {
                Status::Success
            } else {
                Status::Unrecognized
            },
            metadata: file
                .map(|file| file.metadata.iter().map(CachedModMeta::capture).collect())
                .transpose()?
                .unwrap_or_default(),
            errors: result
                .errors
                .iter()
                .map(|error| LoadError {
                    loader: error
                        .loader
                        .as_ref()
                        .map(|loader| loader.id().as_str().to_owned()),
                    error: error.error.clone(),
                })
                .collect(),
        })
    }

    pub fn from_selected(selected: &SelectedFile) -> Result<Self, MetadataConversionError> {
        let scanned = &selected.scanned;
        let file = &scanned.file;
        Ok(Self {
            path: file.entry.path().to_string_lossy().into_owned(),
            name: Some(file.entry.name().to_owned()),
            enabled: Some(matches!(file.entry.status(), ModStatus::Enabled)),
            status: if !scanned.failures.is_empty() {
                Status::Failed
            } else if file.metadata.iter().any(ModMeta::has_mods) {
                Status::Success
            } else {
                Status::Unrecognized
            },
            metadata: file
                .metadata
                .iter()
                .map(CachedModMeta::capture)
                .collect::<Result<_, _>>()?,
            errors: scanned
                .failures
                .iter()
                .map(|failure| LoadError {
                    loader: Some(failure.loader.id().as_str().to_owned()),
                    error: failure.error.clone(),
                })
                .collect(),
            records: selected.records.clone(),
        })
    }

    pub fn failed_input(path: &Path, error: &anyhow::Error) -> Self {
        Self {
            records: None,
            path: path.to_string_lossy().into_owned(),
            name: None,
            enabled: None,
            status: Status::Failed,
            metadata: Vec::new(),
            errors: vec![LoadError {
                loader: None,
                error: ErrorReport::capture(error),
            }],
        }
    }

    /// Rebuilds the presentation input for this record.
    ///
    /// Returns `None` for an input that could not be read; the caller counts those
    /// separately. Diagnostics were already emitted as operation events.
    pub fn into_selected(self) -> Result<Option<SelectedFile>> {
        let mut record = self;
        ensure!(
            (record.status == Status::Failed) != record.errors.is_empty(),
            "list record status and errors disagree for {:?}",
            record.path
        );
        let mut failures = Vec::new();
        for error in std::mem::take(&mut record.errors) {
            if record.name.is_some() {
                failures.push(FailedIdentification {
                    loader: loader(
                        error
                            .loader
                            .as_deref()
                            .context("missing loader ID in identification failure")?,
                    )?,
                    error: error.error,
                });
            }
        }
        let Some(name) = record.name else {
            ensure!(
                record.status == Status::Failed
                    && record.enabled.is_none()
                    && record.metadata.is_empty(),
                "invalid input failure record for {:?}",
                record.path
            );
            return Ok(None);
        };
        let enabled = record
            .enabled
            .context("missing enabled state in mod record")?;
        let suffix = if enabled { ".jar" } else { ".jar.disabled" };
        let filename = format!("{name}{suffix}");
        let entry = ModEntry::from_parts(PathBuf::from(&record.path), ModEntryName::new(filename)?);
        let metadata: Vec<_> = record
            .metadata
            .iter()
            .map(|metadata| metadata.to_mod_meta(&BUILTIN_LOADERS))
            .collect::<Result<_, _>>()
            .with_context(|| format!("invalid mod metadata for {:?}", record.path))?;
        if let Some(keys) = &record.records {
            ensure!(!keys.is_empty(), "empty selected declaration list");
            for key in keys {
                let meta = metadata
                    .get(key.metadata)
                    .context("invalid selected metadata index")?;
                ensure!(
                    key.record < declarations::collect(meta, "").len(),
                    "invalid selected declaration index"
                );
            }
        }
        Ok(Some(SelectedFile {
            scanned: Arc::new(ScannedFile {
                file: ModFile { entry, metadata },
                failures,
            }),
            records: record.records,
        }))
    }
}

fn loader(id: &str) -> Result<ModLoaderMeta> {
    BUILTIN_LOADERS
        .get(id)
        .cloned()
        .with_context(|| format!("unsupported loader {id:?}"))
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    Unrecognized,
    Failed,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LoadError {
    pub loader: Option<String>,
    #[serde(flatten)]
    pub error: ErrorReport,
}

/// Totals of one list acquisition, independent of presentation filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSummary {
    pub files: usize,
    pub input_errors: usize,
    pub failed: usize,
    pub cache: Option<CacheSummary>,
}
