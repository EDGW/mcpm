//! Shared source-identification request, indexed events and provider-neutral result models.

use mcpm_backend::source::types::FileMatch;
use mcpm_backend::data::runtime::RuntimeEnvironment;

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

/// Where the input bytes live; every target runs the same digest operation.
///
/// The session is owned so background TUI work can outlive the calling command frame;
/// cloning a session shares its opened connection.
#[derive(Debug, Clone)]
pub(in crate::commands) enum Location {
    Local {
        session: mcpm_backend::Session,
        environment: RuntimeEnvironment,
    },
    Remote {
        session: mcpm_backend::Session,
        environment: RuntimeEnvironment,
    },
}

impl Location {
    pub(in crate::commands) fn environment(&self) -> &RuntimeEnvironment {
        match self {
            Self::Local { environment, .. } | Self::Remote { environment, .. } => environment,
        }
    }

    pub(in crate::commands) fn session(&self) -> &mcpm_backend::Session {
        match self {
            Self::Local { session, .. } | Self::Remote { session, .. } => session,
        }
    }

    pub(in crate::commands) fn is_local(&self) -> bool {
        matches!(self, Self::Local { .. })
    }
}
