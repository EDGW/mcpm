//! Shared command execution policy, independent of individual command implementations.

pub(super) mod blocking;

mod runtime;
pub(super) use runtime::Runtime;
mod session;
pub(super) use session::{
    default_executable, failure, finish, local_session, session_for, session_for_workspace,
    surface_failure,
};

/// Default bounded concurrency for command work; callers may override it independently.
pub(super) const DEFAULT_PARALLELISM: usize = 10;

/// Nonzero default for adapters that encode bounded concurrency.
pub(super) fn default_parallelism() -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap()
}
