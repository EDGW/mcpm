//! Multi-directory checksum dispatch with shared input and presentation policies.

use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::local::mcmod::backup;

use super::{DisplayArgs, input};
use crate::{commands::execution::default_parallelism, display::backup::Presentation};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Supply directories as arguments, one per nonblank stdin line, or both. Paths are literal: spaces are preserved and stdin quotes are not interpreted. Identical paths are processed once, in first-seen order. At least one directory is required.\n\nRecursively hashes all regular files, including disabled mods and other extensions, using the backup checksum rules. Relative paths are normalized to /; empty directories are omitted. Symbolic links, special files and invalid paths are rejected. No backup or cache is created or modified. Directories are processed sequentially, continuing after failures; --parallelism limits concurrent files within each directory (default 10). File changes detected during reading or file-set changes detected at completion fail the calculation; this is not an atomic directory snapshot. Any failure returns a nonzero exit code.\n\nRaw emits only one checksum per successful directory, one per line, with no paths, statuses or sizes. Failed directories produce no stdout line; diagnostics go to stderr. --headers adds a single checksum header. Console also shows directory, status, file count and content size (without a manifest); --details adds per-file fields in Console only."
)]
pub(super) struct ChecksumArgs {
    /// Directories; redirected stdin appends one directory per nonblank line.
    #[arg(value_name = "DIRECTORY", num_args = 0..)]
    directories: Vec<PathBuf>,
    /// Maximum concurrent files within each directory; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: ChecksumArgs) -> Result<()> {
    let directories = input::directories(args.directories)?;
    let mut display = Presentation::new(
        io::stdout().lock(),
        args.display.details,
        args.display.headers,
    );
    let mut failed = 0_usize;
    for path in directories {
        let result = backup::checksum(&path, args.parallelism, |event| display.event(event))
            .with_context(|| format!("failed to compute directory checksum for {path:?}"));
        display.checksummed(&path, result.as_ref().ok())?;
        if let Err(error) = result {
            failed += 1;
            crate::diagnostics::error(&error);
        }
    }
    ensure!(
        failed == 0,
        "{failed} directory checksums could not be computed"
    );
    Ok(())
}
