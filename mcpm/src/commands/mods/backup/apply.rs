//! CLI policy for applying a verified backup without consulting the content cache.

use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::mcmod::backup::{self, ApplyOptions};

use super::DisplayArgs;
use crate::{
    commands::{cache_support::FileStrategy, execution::default_parallelism},
    display::backup::Presentation,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (one TSV summary on success). Verifies the manifest checksum, complete file set and every file's BLAKE3 before applying. The backup is self-contained; the original cache is not required. Empty directories are not restored.\n\nTARGET must be absent or empty with an existing parent. --force replaces the entire existing directory, removing files absent from the backup. It never skips verification. Files are prepared and verified in a sibling staging directory before replacement; the previous directory is retained until publication succeeds. Overlapping backup/target paths, symbolic links and special backup files are rejected. Copy is the default and creates writable files; hard links share the backup's write protection. prefer-hard-link falls back only when hard links are unsupported."
)]
pub(super) struct ApplyArgs {
    /// Existing backup directory containing manifest.mcbackup and data/.
    backup: PathBuf,
    /// Destination directory; its parent must exist.
    target: PathBuf,
    /// Copy, require a hard link, or copy when hard links are unsupported.
    #[arg(long, value_enum, default_value = "copy")]
    mode: FileStrategy,
    /// Replace the entire nonempty target directory after verification succeeds.
    #[arg(long)]
    force: bool,
    /// Maximum concurrent files in each stage; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: ApplyArgs) -> Result<()> {
    let mut display = Presentation::new(
        io::stdout().lock(),
        args.display.details,
        args.display.headers,
    );
    let outcome = backup::apply(
        &args.backup,
        &args.target,
        ApplyOptions {
            mode: args.mode.into(),
            parallelism: args.parallelism,
            force: args.force,
        },
        |event| display.event(event),
    )
    .with_context(|| {
        format!(
            "failed to apply backup {:?} to {:?}",
            args.backup, args.target
        )
    })?;
    display.finish(&outcome)
}
