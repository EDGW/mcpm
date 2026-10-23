//! Inspection data shared by acquisition and presentation, without filesystem handles.

use kako_mods_lib::local::mcmod::{ModInfo, ModMeta, ModStatus};
use serde::{Deserialize, Serialize};

use super::super::{mod_records, selection::SelectedFile};
use crate::diagnostics::ErrorReport;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(in crate::commands) enum Event {
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
        cache: Option<super::super::scan::CacheSummary>,
    },
}

/// Filenames are parsed on the owning host so Windows paths render correctly on Unix.
#[derive(Debug, Serialize, Deserialize)]
pub(in crate::commands) struct File {
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
                    let records =
                        mod_records::selected(metadata, index, "", selected.records.as_deref());
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

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::commands) struct Metadata {
    pub loader: String,
    pub loader_name: String,
    pub records: Vec<Declaration>,
}

impl Metadata {
    pub(in crate::commands) fn from_mod_meta(metadata: &ModMeta) -> Self {
        Self::from_records(metadata, mod_records::collect(metadata, ""))
    }

    fn from_records(metadata: &ModMeta, records: Vec<mod_records::ModRecord<'_>>) -> Self {
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

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::commands) struct Declaration {
    pub info: ModInfo,
    pub source: Vec<String>,
}
