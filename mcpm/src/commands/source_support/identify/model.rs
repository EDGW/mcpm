//! Shared source-identification request, indexed events and provider-neutral result models.

use kako_mods_lib::source::FileMatch;

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
    /// Verified source-cache reuse for one physical file, before provider requests complete.
    Cache {
        indices: Vec<usize>,
        sources: Vec<usize>,
    },
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

/// Where the input bytes live; remote acquisition reuses a caller-owned session.
#[derive(Debug, Clone, Copy)]
pub(in crate::commands) enum Location<'a> {
    Local,
    Remote(&'a crate::remote::client::Session),
}
impl Location<'_> {
    pub(in crate::commands) fn is_local(self) -> bool {
        matches!(self, Self::Local)
    }
}
