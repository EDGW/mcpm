//! Multi-backup verification policy with shared literal path input and library validation.

use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::local::mcmod::backup;

use super::{DisplayArgs, input};
use crate::{commands::execution::default_parallelism, display::backup::Presentation};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout). Supply backup directory paths as arguments, one per nonblank stdin line, or both. Stdin paths are literal: spaces are preserved and shell quotes are not interpreted. Repeated identical paths are checked once, in first-seen order. At least one path is required.\n\nChecks manifest format, checksum, the complete data/ file set, file sizes and full BLAKE3 hashes without extracting or applying files. The cache is not opened or populated. Backups are processed sequentially; --parallelism limits concurrent files within each backup (default 10). Every selected backup is checked even after failures. Results retain input order; any failure returns a nonzero exit code with detailed diagnostics on stderr. Raw emits one row per backup: backup, status, files, size and checksum; unavailable fields are empty on failure. Size includes the manifest. --details adds file fields in Console only."
)]
pub(super) struct VerifyArgs {
    /// Backup directories; redirected stdin appends one directory per nonblank line.
    #[arg(value_name = "BACKUP", num_args = 0..)]
    backups: Vec<PathBuf>,
    /// Maximum concurrent files within each backup; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: VerifyArgs) -> Result<()> {
    let backups = input::directories(args.backups)?;
    let mut display = Presentation::new(
        io::stdout().lock(),
        args.display.details,
        args.display.headers,
    );
    let mut failed = 0_usize;
    for path in backups {
        let result = backup::verify(&path, args.parallelism, |event| display.event(event))
            .with_context(|| format!("failed to verify backup {path:?}"));
        display.verified(&path, result.as_ref().ok())?;
        if let Err(error) = result {
            failed += 1;
            crate::diagnostics::error(&error);
        }
    }
    ensure!(failed == 0, "{failed} backups failed verification");
    Ok(())
}
