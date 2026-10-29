//! Shared command execution policy, independent of individual command implementations.

pub(crate) mod blocking;

mod runtime;
pub(crate) use runtime::Runtime;
mod session;
pub(crate) use session::{
    default_executable, failure, finish, local_session, session_for, session_for_workspace,
    surface_failure,
};

/// Default bounded concurrency for command work; callers may override it independently.
pub(crate) const DEFAULT_PARALLELISM: usize = 10;

/// Nonzero default for adapters that encode bounded concurrency.
pub(crate) fn default_parallelism() -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap()
}
