//! Errors from local client launch preparation, including original I/O and archive causes.

use std::{io, path::PathBuf};

use thiserror::Error;

use crate::local::mc::client::CheckReport;

use super::super::JavaVersionCheck;

#[derive(Debug, Error)]
pub enum PreparationError {
    #[error("{operation}: {path:?}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("required launch file {path:?} is not a regular file")]
    NotFile { path: PathBuf },
    #[error("selected resources did not pass verification")]
    Verification { report: Box<CheckReport> },
    #[error("cannot read native archive {path:?}")]
    Archive {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("native archive {archive:?} has an unsafe entry {entry:?}")]
    UnsafeEntry { archive: PathBuf, entry: String },
    #[error("native extraction exceeds the {maximum_bytes} byte limit")]
    ExtractionLimit { maximum_bytes: u64 },
    #[error("refusing to write through a symlink or non-directory at {path:?}")]
    UnsafeDestination { path: PathBuf },
    #[error(
        "Java version probe {executable:?} in {working_directory:?} failed with {status}; stderr: {stderr:?}; stdout: {stdout:?}"
    )]
    JavaVersionProbe {
        executable: PathBuf,
        working_directory: PathBuf,
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
    #[error(
        "Java version probe {executable:?} returned no recognizable version; stderr: {stderr:?}; stdout: {stdout:?}"
    )]
    JavaVersionUnknown {
        executable: PathBuf,
        stdout: String,
        stderr: String,
    },
    #[error("failed to probe Java {executable:?} in {working_directory:?}")]
    JavaVersionIo {
        executable: PathBuf,
        working_directory: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Java {actual} does not satisfy {policy:?} requirement {required}")]
    JavaVersionMismatch {
        actual: u32,
        required: u32,
        policy: JavaVersionCheck,
    },
}
