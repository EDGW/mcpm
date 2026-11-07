//! CLI parallelism selection; execution and first-failure scheduling belong to Host refresh operations.

use crate::commands::execution::default_parallelism;
use clap::Args;
use std::num::NonZeroUsize;

#[derive(Debug, Args)]
pub(super) struct Parallelism {
    /// Maximum concurrent cache entries with --all; explicit HASH inputs stay sequential.
    #[arg(long, global = true, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
}

impl Parallelism {
    pub(super) fn selected(&self, all: bool) -> usize {
        if all { self.parallelism.get() } else { 1 }
    }
}
