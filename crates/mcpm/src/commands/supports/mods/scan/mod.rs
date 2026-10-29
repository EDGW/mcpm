//! Frontend names for the scan records produced by backend operations.

pub(in crate::commands) use mcpm_backend::mods::scan::{CacheSummary, Event, Options};

// Used by TUI tests to build inspection fixtures.
#[allow(unused_imports)]
pub(in crate::commands) use mcpm_backend::mods::scan::{FailedIdentification, ScannedFile};

mod progress;
pub(in crate::commands) use progress::ProgressState;
