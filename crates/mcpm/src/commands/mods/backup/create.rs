//! CLI policy for creating a content-only Mod backup through the library workflow.

use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::{
    cache::mods::ModsCacheSelection,
    mcmod::backup::{self, CreateOptions},
    runtime::RuntimeEnvironment,
};

use super::DisplayArgs;
use crate::{
    commands::{cache_support::FileStrategy, execution::default_parallelism},
    display::backup::{self as display, Presentation},
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (one TSV summary on success).\n\n- SOURCE must exist. BACKUP must be absent or empty with an existing parent. Source, backup and cache must not overlap.\n- Recursively includes every regular file, including disabled mods and non-JAR files. Empty directories are omitted; symbolic links, special files and invalid paths are rejected.\n- Copies source content into the cache without computing meta or ident. Source files and the required cache contents must pass full verification. Independent source files remain writable.\n- Full backups export protected data files. --mode controls cache-to-backup transfer (default: hard-link); prefer-hard-link falls back only when hard links are unsupported.\n- --cache-only publishes only manifest.mcbackup, without data/. It conflicts with an explicit --mode.\n- Manifest-only backups depend on the referenced cache contents. Cache cleanup, loss or corruption may make restoration impossible. Copying the manifest does not copy the contents or prevent cache removal. Empty manifests have no content dependency.\n- Risk and cache-dependency notices go to stderr, including in Raw and with logging disabled. No extra confirmation is required.\n- Failed backups are discarded; completed cache imports remain. Corrupt cache contents are not automatically repaired."
)]
pub(super) struct CreateArgs {
    /// Existing directory whose files will be backed up recursively.
    source: PathBuf,
    /// New or empty backup directory; its parent must exist.
    backup: PathBuf,
    /// Copy, require a hard link, or copy when hard links are unsupported.
    #[arg(long, value_enum, default_value = "hard-link")]
    mode: FileStrategy,
    /// Save only the manifest; restoring requires cache contents, which cleanup or loss may remove.
    #[arg(long, conflicts_with = "mode")]
    cache_only: bool,
    /// Content cache; explicit directories must already be initialized.
    #[arg(long, value_name = "PATH")]
    cache_dir: Option<PathBuf>,
    /// Maximum concurrent files in each stage; use 1 for sequential processing.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: CreateArgs) -> Result<()> {
    let selection = args
        .cache_dir
        .map(|path| ModsCacheSelection::Directory { path })
        .unwrap_or(ModsCacheSelection::Default);
    if args.cache_only {
        display::cache_only_warning()?;
    }
    let environment = RuntimeEnvironment::new().with_backup_cache(selection);
    let mut display = Presentation::new(
        io::stdout().lock(),
        args.display.details,
        args.display.headers,
    );
    let outcome = backup::create(
        &args.source,
        &args.backup,
        &environment,
        CreateOptions {
            storage: if args.cache_only {
                backup::CreateStorage::CacheOnly
            } else {
                backup::CreateStorage::Full(args.mode.into())
            },
            parallelism: args.parallelism,
        },
        |event| display.event(event),
    )
    .with_context(|| format!("failed to create backup {:?}", args.backup))?;
    display.finish(&outcome)?;
    if args.cache_only {
        display::cache_only_created(
            environment.cache.backup.open()?.path(),
            outcome.manifest().files().is_empty(),
        )?;
    }
    Ok(())
}
