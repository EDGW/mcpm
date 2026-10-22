//! Shared JSON records for remote list production and SSH consumption.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry, ModInfo, ModMeta, ModStatus},
    modloader::{LoaderId, ModLoaderMeta},
};
use serde::{Deserialize, Serialize};

use crate::{commands::mod_support::loaders, diagnostics::ErrorReport};

/// This payload schema belongs exclusively to the mod-list command.
#[derive(Serialize, Deserialize)]
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
    pub(super) fn load(
        path: PathBuf,
        entry: Result<ModEntry>,
        selected_loaders: &HashMap<LoaderId, ModLoaderMeta>,
    ) -> Self {
        let started = std::time::Instant::now();
        tracing::debug!(path = %path.display(), "Identifying remote mod file");
        let mut record = Self {
            path: path.to_string_lossy().into_owned(),
            name: None,
            enabled: None,
            status: Status::Failed,
            metadata: Vec::new(),
            errors: Vec::new(),
        };
        let result = entry.and_then(|entry| {
            record.name = Some(entry.name().to_owned());
            record.enabled = Some(matches!(entry.status(), ModStatus::Enabled));
            mcmod::open(entry, selected_loaders.clone(), |_, loader, _, error| {
                tracing::debug!(path = %record.path, loader = %loader.id(), error = %format_args!("{error:#}"), "Remote mod loader identification failed");
                record.errors.push(LoadError {
                    loader: Some(loader.id().as_str().to_owned()),
                    error: ErrorReport::capture(error),
                });
                IdentificationErrorAction::Skip
            })
        });
        match result {
            Ok(mut file) => {
                loaders::sort_metadata(&mut file);
                record.status = if !record.errors.is_empty() {
                    Status::Failed
                } else if file.metadata.iter().any(ModMeta::has_mods) {
                    Status::Success
                } else {
                    Status::Unrecognized
                };
                record.metadata = file.metadata.into_iter().map(Metadata::from).collect();
            }
            Err(error) => {
                tracing::debug!(path = %record.path, error = %format_args!("{error:#}"), "Remote mod input failed");
                record.errors.push(LoadError {
                    loader: None,
                    error: ErrorReport::capture(&error),
                });
            }
        }
        record.errors.sort_by(|a, b| a.loader.cmp(&b.loader));
        tracing::debug!(path = %record.path, status = ?record.status, elapsed_ms = started.elapsed().as_millis(), "Remote mod identification completed");
        record
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

impl From<ModMeta> for Metadata {
    fn from(meta: ModMeta) -> Self {
        Self {
            loader: meta.loader.id().as_str().to_owned(),
            mods: meta.mods,
            nested: meta
                .nested
                .into_iter()
                .map(|child| NestedMetadata {
                    path: child.path,
                    meta: child.meta.into(),
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
