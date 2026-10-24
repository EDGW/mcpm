//! CLI concurrency policy for inspection directory scans and lazy name/ID indexes.

use std::num::NonZeroUsize;

use clap::Args;

use crate::commands::execution::default_parallelism;

#[derive(Args, Debug)]
pub(in crate::commands) struct LoadingArgs {
    /// Parallel file reads for --all and directory indexing by modid/name (local or remote).
    /// Multiple independent file inputs are processed sequentially; this option does not parallelize them.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
}

impl LoadingArgs {
    pub(in crate::commands) fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
}
