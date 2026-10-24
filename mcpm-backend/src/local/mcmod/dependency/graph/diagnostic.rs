//! Diagnostic records retained by dependency graphs and their snapshots.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A skipped identification failure, retained as snapshot diagnostics.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentificationFailure {
    pub path: PathBuf,
    pub loader: String,
    /// Context and cause chain supplied by the loader.
    pub error: String,
}
