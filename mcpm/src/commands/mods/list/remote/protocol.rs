//! Shared JSON records for remote list production and SSH consumption.

use kako_mods_lib::local::mcmod::{ModInfo, ModMeta, ModStatus};
use serde::{Deserialize, Serialize};

use crate::{commands::mod_support::scan::ScanResult, diagnostics::ErrorReport};

/// This payload schema belongs exclusively to the mod-list command.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(in crate::commands::mods::list) enum Contents {
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
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands::mods::list) struct Record {
    pub(in crate::commands::mods::list) path: String,
    pub(in crate::commands::mods::list) name: Option<String>,
    pub(in crate::commands::mods::list) enabled: Option<bool>,
    pub(in crate::commands::mods::list) status: Status,
    pub(in crate::commands::mods::list) metadata: Vec<Metadata>,
    pub(in crate::commands::mods::list) errors: Vec<LoadError>,
}

impl Record {
    pub(super) fn from_scan(path: &std::path::Path, result: &ScanResult) -> Self {
        let file = result.file.as_ref().map(|scanned| &scanned.file);
        Self {
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
                .map(|file| file.metadata.iter().map(Metadata::from).collect())
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
        }
    }

    pub(super) fn failed_input(path: &std::path::Path, error: &anyhow::Error) -> Self {
        Self {
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
pub(in crate::commands::mods::list) enum Status {
    Success,
    Unrecognized,
    Failed,
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands::mods::list) struct Metadata {
    pub(in crate::commands::mods::list) loader: String,
    pub(in crate::commands::mods::list) mods: Vec<ModInfo>,
    pub(in crate::commands::mods::list) nested: Vec<NestedMetadata>,
}

impl From<&ModMeta> for Metadata {
    fn from(meta: &ModMeta) -> Self {
        Self {
            loader: meta.loader.id().as_str().to_owned(),
            mods: meta.mods.clone(),
            nested: meta
                .nested
                .iter()
                .map(|child| NestedMetadata {
                    path: child.path.clone(),
                    meta: (&child.meta).into(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands::mods::list) struct NestedMetadata {
    pub(in crate::commands::mods::list) path: String,
    pub(in crate::commands::mods::list) meta: Metadata,
}

#[derive(Serialize, Deserialize, Debug)]
pub(in crate::commands::mods::list) struct LoadError {
    pub(in crate::commands::mods::list) loader: Option<String>,
    #[serde(flatten)]
    pub(in crate::commands::mods::list) error: ErrorReport,
}
