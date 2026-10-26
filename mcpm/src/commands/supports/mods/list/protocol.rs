//! Shared JSON records for remote list production and SSH consumption.

use mcpm_backend::local::mcmod::{CachedModMeta, MetadataConversionError, ModMeta, ModStatus};
use serde::{Deserialize, Serialize};

use crate::{commands::supports::mods::scan::ScanResult, diagnostics::ErrorReport};

/// List wire records shared by path-based and server-scoped adapters.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(in crate::commands) enum Contents {
    Count {
        count: usize,
    },
    Item {
        index: usize,
        record: Record,
    },
    Error {
        #[serde(flatten)]
        error: ErrorReport,
    },
    Done {
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<crate::commands::supports::mods::scan::CacheSummary>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands) struct Record {
    pub(in crate::commands) path: String,
    pub(in crate::commands) name: Option<String>,
    pub(in crate::commands) enabled: Option<bool>,
    pub(in crate::commands) status: Status,
    pub(in crate::commands) metadata: Vec<CachedModMeta>,
    pub(in crate::commands) errors: Vec<LoadError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::commands) records:
        Option<Vec<crate::commands::supports::mods::mod_records::RecordKey>>,
}

impl Record {
    pub(in crate::commands) fn from_scan(
        path: &std::path::Path,
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

    pub(in crate::commands) fn from_selected(
        selected: &crate::commands::supports::mods::selection::SelectedFile,
    ) -> Result<Self, MetadataConversionError> {
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

    pub(in crate::commands) fn failed_input(path: &std::path::Path, error: &anyhow::Error) -> Self {
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
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Status {
    Success,
    Unrecognized,
    Failed,
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands) struct LoadError {
    pub(in crate::commands) loader: Option<String>,
    #[serde(flatten)]
    pub(in crate::commands) error: ErrorReport,
}
