//! Shared bounded file parallelism for stash operations; command-specific behavior stays in leaf modules.

use std::num::NonZeroUsize;

use clap::Args;

#[derive(Debug, Args)]
pub(super) struct WorkArgs {
    /// Maximum concurrent files per stage; positive, default 10.
    #[arg(long, default_value_t = crate::commands::execution::default_parallelism())]
    pub parallelism: NonZeroUsize,
}
