//! Serializable operation observations; cached metadata itself stays inside the library.

use super::super::identification::{QueryResult, QueryStatus};
use super::{FileResult, Stage};
use mcpm_exec::error::PortableError;
use mcpm_lib::local::cache::TransferMethod;
use mcpm_lib::local::cache::mods::CacheStatus;
use mcpm_lib::local::cache::mods::CacheUnavailableReason;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub path: PathBuf,
    pub id: Option<String>,
    pub published: Option<bool>,
    pub method: Option<Method>,
    pub status: Option<Status>,
    pub reason: Option<String>,
    pub data_path: Option<PathBuf>,
    pub digest_path: Option<PathBuf>,
    pub metadata_requested: bool,
    pub identification: Option<QueryResult>,
    pub failed_stage: Option<Stage>,
    pub error: Option<Arc<PortableError>>,
    pub identification_error: Option<Arc<PortableError>>,
    pub publication_errors: Vec<PublicationFailure>,
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
            identification: None,
            failed_stage: file.failure.as_ref().map(|failure| failure.stage),
            error: file.failure.map(|failure| failure.error),
            identification_error: None,
            publication_errors: Vec::new(),
        }
    }
}

impl Record {
    /// Publication failure never overwrites independent source or metadata failures.
    pub fn overall_status(&self) -> QueryStatus {
        if self.error.is_some()
            || self.identification_error.is_some()
            || !self.publication_errors.is_empty()
        {
            QueryStatus::Failed
        } else {
            self.identification
                .as_ref()
                .map_or(QueryStatus::Success, QueryResult::status)
        }
    }
    pub fn errors(&self) -> impl Iterator<Item = &PortableError> {
        self.error
            .iter()
            .map(Arc::as_ref)
            .chain(self.identification_error.iter().map(Arc::as_ref))
            .chain(self.identification.iter().flat_map(QueryResult::errors))
            .chain(
                self.publication_errors
                    .iter()
                    .map(|failure| failure.error.as_ref()),
            )
    }
}

/// A publication-stage error is independent of the preceding source query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicationFailure {
    pub source: Option<mcpm_lib::source::SourceId>,
    pub error: Arc<PortableError>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
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
pub enum Status {
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
