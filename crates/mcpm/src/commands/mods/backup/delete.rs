//! Multi-backup deletion dispatch with shared input policy and independent diagnostics.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_backend::local::mcmod::backup;

use super::input;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout). Supply backup directory paths as arguments, one per nonblank stdin line, or both. Stdin paths are literal: spaces are preserved and shell quotes are not interpreted. Identical paths are deleted once, in first-seen order. At least one path is required.\n\nDeletes the entire backup directory, including data/ and manifest.mcbackup. Requires a valid manifest and rejects unrelated top-level entries and symbolic-link roots. Data contents need not pass verification; missing data/ is allowed. All contents inside data/ are removed, including unlisted files; symbolic links inside it are unlinked without following their targets. Only directory protection is temporarily relaxed; cached content and other hard links retain their permissions. No cache is opened. An I/O failure may leave a partially deleted backup.\n\nBackups are processed sequentially, continuing after individual failures. Any failure returns a nonzero exit code with detailed diagnostics on stderr. Raw emits one row per backup: backup and status (deleted or failed)."
)]
pub(super) struct DeleteArgs {
    /// Backup directories; redirected stdin appends one directory per nonblank line.
    #[arg(value_name = "BACKUP", num_args = 0..)]
    backups: Vec<PathBuf>,
    /// Include the header in Raw TSV output; ignored by Console.
    #[arg(long)]
    headers: bool,
}

pub(super) fn run(args: DeleteArgs) -> Result<()> {
    let backups = input::directories(args.backups)?;
    let mut output = io::stdout().lock();
    let mut failed = 0_usize;
    for (index, path) in backups.into_iter().enumerate() {
        let result =
            backup::delete(&path).with_context(|| format!("failed to delete backup {path:?}"));
        crate::display::backup::deletion_result(
            &mut output,
            &path,
            result.is_ok(),
            args.headers && index == 0,
        )?;
        output.flush()?;
        if let Err(error) = result {
            failed += 1;
            crate::diagnostics::error(&error);
        }
    }
    ensure!(failed == 0, "{failed} backups could not be deleted");
    Ok(())
}
