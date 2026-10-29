//! Serializable operation observations; cached metadata itself stays inside the library.

use super::{FileResult, Stage};
use crate::diagnostics::ErrorReport;
use mcpm_backend::local::cache::TransferMethod;
use mcpm_backend::local::cache::mods::CacheStatus;
use mcpm_backend::local::cache::mods::CacheUnavailableReason;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct Record {
    pub path: PathBuf,
    pub id: Option<String>,
    pub published: Option<bool>,
    pub method: Option<Method>,
    pub status: Option<Status>,
    pub reason: Option<String>,
    pub data_path: Option<PathBuf>,
    pub digest_path: Option<PathBuf>,
    #[serde(default = "default_metadata_requested")]
    pub metadata_requested: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identification: BTreeMap<String, IdentResult>,
    pub success: bool,
    pub failed_stage: Option<Stage>,
    pub error: Option<ErrorReport>,
    #[serde(default)]
    pub error_remote: bool,
}
impl From<FileResult> for Record {
    fn from(file: FileResult) -> Self {
        let (status, reason) = match file.entry.as_ref().map(|entry| entry.status()) {
            Some(CacheStatus::Available) => (Some(Status::Available), None),
            Some(CacheStatus::NotFound) => (Some(Status::NotFound), None),
            Some(CacheStatus::Unavailable(reason)) => (
                Some(Status::Unavailable),
                Some(match reason {
                    CacheUnavailableReason::MissingDigest => "missing_digest".to_owned(),
                    CacheUnavailableReason::InvalidData => "invalid_data".to_owned(),
                    CacheUnavailableReason::UnexpectedError(error) => error.clone(),
                }),
            ),
            None => (None, None),
        };
        Self {
            path: file.path,
            id: file.import.map(|import| import.key.to_string()),
            published: file.import.map(|import| import.published),
            method: file
                .import
                .and_then(|import| import.method)
                .map(|method| match method {
                    TransferMethod::Copy => Method::Copy,
                    TransferMethod::HardLink => Method::HardLink,
                }),
            status,
            reason,
            data_path: file
                .entry
                .as_ref()
                .map(|entry| entry.data_path().to_owned()),
            digest_path: file
                .entry
                .as_ref()
                .map(|entry| entry.digest_path().to_owned()),
            metadata_requested: file.metadata_requested,
            identification: BTreeMap::new(),
            success: file.failure.is_none(),
            failed_stage: file.failure.as_ref().map(|failure| failure.stage),
            error: file.failure.map(|failure| failure.error),
            error_remote: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Method {
    Copy,
    HardLink,
}
impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::HardLink => "hard_link",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Status {
    Available,
    NotFound,
    Unavailable,
}
impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
        }
    }
}

fn default_metadata_requested() -> bool {
    true
}

/// Each requested source has its own outcome; a miss is not a persistent negative cache entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::commands) enum IdentResult {
    Matched,
    Unmatched,
    Failed {
        error: ErrorReport,
        #[serde(default)]
        remote: bool,
    },
}

impl IdentResult {
    pub(super) fn status(&self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Unmatched => "unmatched",
            Self::Failed { .. } => "failed",
        }
    }
}
