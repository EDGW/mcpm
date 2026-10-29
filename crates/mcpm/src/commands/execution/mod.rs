//! Shared command execution policy, independent of individual command implementations.

pub(super) mod blocking;

mod runtime;
pub(super) use runtime::Runtime;

/// Default bounded concurrency for command work; callers may override it independently.
pub(super) const DEFAULT_PARALLELISM: usize = 10;

/// Nonzero default for adapters that encode bounded concurrency.
pub(super) fn default_parallelism() -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap()
}
