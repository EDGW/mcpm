//! Shared source-identification data: one file summary per physical file and its outcome.
//!
//! Summaries are computed by the host that owns the bytes; the operation never reads a
//! file. Verified cache matches travel with the summary so a hit needs no provider request.

use std::{collections::BTreeMap, path::PathBuf};

use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::{FileMatch, FileSummary, SummaryRequirements};

/// One physical file, the input indices that share it, and its verified cache matches.
#[derive(Debug, Clone)]
pub struct IdentifyFile {
    /// Original input position, used by the caller to restore its own ordering.
    pub indices: Vec<usize>,
    /// Diagnostic label of the summarized file; never read by this operation.
    pub path: PathBuf,
    pub summary: FileSummary,
    /// Verified source matches read from the file host's cache, keyed by cache identity.
    pub cached: BTreeMap<String, FileMatch>,
}

/// Result of identifying one physical file against one source.
#[derive(Debug, Clone)]
pub enum IdentifyOutcome {
    Matched {
        /// Match in the configured source's namespace, for provider calls and display.
        file: Box<FileMatch>,
        /// The same match in the shared cache namespace, for persistent identification.
        cache: Box<FileMatch>,
    },
    Unmatched,
    Failed { error: ErrorReport },
}

/// One configured source's cache identity and summary requirements.
///
/// The profile is observed before any request: the digest needs the requirements and a
/// cache writer needs the stable cache identity. A specification that cannot be built is
/// reported per file by the identify operation, so the profile carries its error instead
/// of failing the whole pipeline.
#[derive(Debug, Clone)]
pub struct SourceProfile {
    /// Namespace of verified content matches; never contains credentials or endpoints.
    pub cache_key: String,
    /// Filled only when the provider could be built.
    pub requirements: SummaryRequirements,
    /// Provider construction failure, or `None` when the source is usable.
    pub error: Option<ErrorReport>,
}
