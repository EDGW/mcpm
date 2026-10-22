//! Shared source-identification request, indexed events and provider-neutral result models.

use kako_mods_lib::source::FileMatch;

/// Shared default for standalone identification and mod-list source lookups.
pub(in crate::commands) const DEFAULT_PARALLELISM: usize = 10;

/// Business result shared by CLI output, TUI state and remote messages.
#[derive(Debug, Clone)]
pub(in crate::commands) enum Outcome {
    Matched {
        file: FileMatch,
    },
    Unmatched,
    Failed {
        remote: bool,
        error: crate::diagnostics::ErrorReport,
    },
}
impl Outcome {
    pub(super) fn status(&self) -> &'static str {
        match self {
            Self::Matched { .. } => "matched",
            Self::Unmatched => "unmatched",
            Self::Failed { .. } => "failed",
        }
    }
}

/// Indices refer to the original inputs; duplicate physical files share one lookup.
#[derive(Debug, Clone)]
pub(in crate::commands) enum Event {
    Loading {
        indices: Vec<usize>,
        source: usize,
    },
    Item {
        indices: Vec<usize>,
        source: usize,
        outcome: Outcome,
    },
    Done {
        failed: usize,
    },
}
