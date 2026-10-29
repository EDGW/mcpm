//! Typed comparison target inputs and one directory calculation shared by all targets.

use std::{collections::HashSet, io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use mcpm_backend::local::mcmod::backup::{self, Comparison, DirectoryChecksum};

use super::DisplayArgs;
use crate::{
    commands::{execution::default_parallelism, stdin},
    display::backup::Presentation,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout). Provide a mods directory and at least one --backup or --checksum target. Repeat these options or use --backups / --checksums with comma-separated values. Nonblank stdin lines append targets and require --input backup or --input checksum; types are never inferred. Stdin values are literal, preserving spaces and quotes. Identical targets of the same type are compared once. Order is --backup, --backups, --checksum, --checksums, then stdin, preserving order within each group.\n\nThe directory is hashed once for all targets, using the same complete file set, portable relative paths and full BLAKE3 rules as backup checksum. No cache is consulted and no files are written. --backup validates only the manifest, without reading data/; use backup verify to check the backup itself. A manifest permits missing, unexpected and changed file details; a bare checksum permits only checksum comparison. Checksums must be 64 lowercase hexadecimal characters. Individual target failures do not stop later comparisons. Any mismatch or failure returns a nonzero exit code.\n\nRaw emits target_type, target, directory, status (match, mismatch or failed), expected_checksum and actual_checksum; unavailable checksums are empty on failure. --headers adds the TSV header. Console always shows file differences for manifest mismatches; --details additionally lists the computed directory files."
)]
pub(super) struct VerifyWithArgs {
    /// Directory whose files are compared with the selected expectations.
    #[arg(value_name = "MODS_DIR")]
    directory: PathBuf,
    /// Select one backup directory; repeat to compare more. Commas are literal.
    #[arg(long, value_name = "BACKUP")]
    backup: Vec<PathBuf>,
    /// Select comma-separated backup directories; combined with --backup.
    #[arg(long, value_name = "BACKUPS", value_delimiter = ',')]
    backups: Vec<PathBuf>,
    /// Select one backup-compatible checksum; repeat to compare more.
    #[arg(long, value_name = "CHECKSUM")]
    checksum: Vec<String>,
    /// Select comma-separated checksums; combined with --checksum.
    #[arg(long, value_name = "CHECKSUMS", value_delimiter = ',')]
    checksums: Vec<String>,
    /// Interpret stdin lines as backup paths or checksums; required for nonempty stdin.
    #[arg(long, value_name = "TYPE")]
    input: Option<InputType>,
    /// Maximum concurrent files in the directory; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: VerifyWithArgs) -> Result<()> {
    let targets = targets(&args)?;
    let mut display = Presentation::new(
        io::stdout().lock(),
        args.display.details,
        args.display.headers,
    );
    let computed = backup::checksum(&args.directory, args.parallelism, |event| {
        display.event(event)
    })
    .with_context(|| {
        format!(
            "failed to compute directory checksum for {:?}",
            args.directory
        )
    });
    let mut failed = 0_usize;
    let mut mismatched = 0_usize;
    for target in targets {
        let result = computed
            .as_ref()
            .ok()
            .map(|computed| target.compare(computed));
        display.compared(
            (target.kind(), &target.value()),
            &args.directory,
            computed.as_ref().ok(),
            result.as_ref().and_then(|result| result.as_ref().ok()),
        )?;
        match result {
            Some(Ok(comparison)) if !comparison.matches() => mismatched += 1,
            Some(Err(error)) => {
                failed += 1;
                crate::diagnostics::error(&error);
            }
            None => failed += 1,
            Some(Ok(_)) => {}
        }
    }
    if let Err(error) = computed {
        crate::diagnostics::error(&error);
    }
    ensure!(
        failed == 0 && mismatched == 0,
        "{mismatched} targets did not match; {failed} targets could not be compared"
    );
    Ok(())
}

fn targets(args: &VerifyWithArgs) -> Result<Vec<Target>> {
    let mut targets: Vec<_> = args
        .backup
        .iter()
        .chain(&args.backups)
        .cloned()
        .map(Target::Backup)
        .chain(
            args.checksum
                .iter()
                .chain(&args.checksums)
                .cloned()
                .map(Target::Checksum),
        )
        .collect();
    let lines = stdin::lines()?;
    if !lines.is_empty() {
        let input = args
            .input
            .context("nonempty stdin requires --input backup or --input checksum")?;
        targets.extend(lines.into_iter().map(|value| match input {
            InputType::Backup => Target::Backup(value.into()),
            InputType::Checksum => Target::Checksum(value),
        }));
    }
    ensure!(
        !targets.is_empty(),
        "provide at least one --backup, --checksum or explicitly typed stdin target"
    );
    let mut seen = HashSet::new();
    targets.retain(|target| seen.insert(target.clone()));
    Ok(targets)
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputType {
    Backup,
    Checksum,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Target {
    Backup(PathBuf),
    Checksum(String),
}

impl Target {
    fn compare(&self, computed: &DirectoryChecksum) -> Result<Comparison> {
        match self {
            Self::Backup(path) => computed
                .compare_with_backup(path)
                .with_context(|| format!("failed to compare with backup {path:?}")),
            Self::Checksum(value) => Ok(computed.compare_checksum(
                value
                    .parse()
                    .with_context(|| format!("invalid expected checksum {value:?}"))?,
            )),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Backup(_) => "backup",
            Self::Checksum(_) => "checksum",
        }
    }

    fn value(&self) -> String {
        match self {
            Self::Backup(path) => path.to_string_lossy().into_owned(),
            Self::Checksum(value) => value.clone(),
        }
    }
}
