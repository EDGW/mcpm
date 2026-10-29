//! CLI policy for applying verified backup content with lazy cache fallback for missing files.

use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::{
    cache::mods::{ModsCacheDir, ModsCacheSelection},
    mcmod::backup::{self, ApplyOptions},
};

use super::DisplayArgs;
use crate::{
    commands::{cache_support::FileStrategy, execution::default_parallelism},
    display::backup::Presentation,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (one TSV summary on success).\n\n- Verifies the manifest checksum, file set, sizes and complete BLAKE3 contents before applying. Empty directories are not restored.\n- Uses backup files first. Only missing files fall back to --cache-dir, defaulting to the global cache (~/.mcpm/cache/mods).\n- Corrupt, unreadable or invalid-type backup files and unexpected files are errors, without cache fallback. Required cache contents must exist and pass full verification.\n- Complete backups and empty manifests do not open or require a cache. Apply never initializes a cache, repairs the backup, searches other caches or the network, or populates cache contents.\n- TARGET must be absent or empty with an existing parent. --force replaces the entire directory, removes extra target files and never skips verification.\n- Files are prepared and verified in a sibling staging directory before replacement. The previous directory is retained until publication succeeds; ordinary publication failures attempt rollback.\n- Backup and target must not overlap. A fallback cache must not overlap either. Symbolic links and special backup files are rejected.\n- Copy is the default and creates writable files. Hard links share their backup or cache source's write protection; prefer-hard-link falls back only when hard links are unsupported."
)]
pub(super) struct ApplyArgs {
    /// Existing backup directory containing manifest.mcbackup; missing data files use the cache.
    backup: PathBuf,
    /// Destination directory; its parent must exist.
    target: PathBuf,
    /// Fallback content cache; defaults to global (~/.mcpm/cache/mods). Opened only for missing files.
    #[arg(long, value_name = "PATH")]
    cache_dir: Option<PathBuf>,
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
    let selection = args
        .cache_dir
        .map(|path| ModsCacheSelection::Directory { path })
        .unwrap_or(ModsCacheSelection::Default);
    let outcome = backup::apply_with_cache(
        &args.backup,
        &args.target,
        || ModsCacheDir::new(selection.path()?),
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
