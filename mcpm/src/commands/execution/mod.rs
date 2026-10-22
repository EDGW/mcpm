//! Shared command execution policy, independent of individual command implementations.

mod runtime;
pub(super) use runtime::Runtime;

/// Default bounded concurrency for command work; callers may override it independently.
pub(super) const DEFAULT_PARALLELISM: usize = 10;
