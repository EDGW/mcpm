//! Inspection data shared by acquisition and presentation, without filesystem handles.

use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::local::mcmod::{ModInfo, ModMeta, ModStatus};
use serde::{Deserialize, Serialize};

use super::declarations::{self, ModRecord};
use crate::mods::scan::CacheSummary;
use crate::mods::selection::SelectedFile;

/// One inspected file, already narrowed to the selected declarations.
#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub filename: String,
    pub enabled: bool,
    pub metadata: Vec<Metadata>,
    pub failed_loaders: Vec<String>,
}

impl From<&SelectedFile> for File {
    fn from(selected: &SelectedFile) -> Self {
        let file = &selected.scanned.file;
        Self {
            path: file.entry.path().to_string_lossy().into_owned(),
            filename: file.entry.filename().to_owned(),
            enabled: matches!(file.entry.status(), ModStatus::Enabled),
            metadata: file
                .metadata
                .iter()
                .enumerate()
                .filter_map(|(index, metadata)| {
                    let records = declarations::selected(
                        metadata,
                        index,
                        "",
                        selected.records.as_deref(),
                    );
                    if selected.records.is_some() && records.is_empty() {
                        return None;
                    }
                    Some(Metadata::from_records(metadata, records))
                })
                .collect(),
            failed_loaders: selected
                .scanned
                .failures
                .iter()
                .map(|failure| failure.loader.id().as_str().to_owned())
                .collect(),
        }
    }
}

/// One loader's declarations inside a file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub loader: String,
    pub loader_name: String,
    pub records: Vec<Declaration>,
}

impl Metadata {
    fn from_records(metadata: &ModMeta, records: Vec<ModRecord<'_>>) -> Self {
        Self {
            loader: metadata.loader.id().as_str().to_owned(),
            loader_name: metadata.loader.name().to_owned(),
            records: records
                .into_iter()
                .map(|record| Declaration {
                    info: record.info.clone(),
                    source: record.source,
                })
                .collect(),
        }
    }
}

/// One recognized declaration and its nesting source path.
#[derive(Debug, Serialize, Deserialize)]
pub struct Declaration {
    pub info: ModInfo,
    pub source: Vec<String>,
}

/// Selection acquisition events; callers render or forward them.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Scan {
        completed: usize,
        total: usize,
    },
    File {
        index: usize,
        file: File,
    },
    Error {
        error: ErrorReport,
    },
    Done {
        files: usize,
        failed: usize,
        ignored: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<CacheSummary>,
    },
}
