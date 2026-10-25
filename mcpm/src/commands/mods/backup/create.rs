//! CLI policy for creating a content-only Mod backup through the library workflow.

use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::{io, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::{
    cache::mods::ModsCacheSelection,
    mcmod::backup::{self, CreateOptions},
};

use super::DisplayArgs;
use crate::{
    commands::{cache_support::FileStrategy, execution::default_parallelism},
    display::backup::Presentation,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (one TSV summary on success). SOURCE must exist. BACKUP must be absent or an empty directory with an existing parent. Recursively backs up every regular file; omits empty directories and rejects symbolic links and special files. Source, backup and cache directories must not overlap.\n\nImports source content by copying into the cache without computing meta or ident. All source files must verify before backup generation starts. --mode controls cache-to-backup transfer; hard-link is the default, and prefer-hard-link falls back only when hard links are unsupported. The source remains writable unless it was already linked to protected content. Backup data is write-protected; the manifest is published last. Failed backups are discarded, while completed cache imports remain."
)]
pub(super) struct CreateArgs {
    /// Existing directory whose files will be backed up recursively.
    source: PathBuf,
    /// New or empty backup directory; its parent must exist.
    backup: PathBuf,
    /// Copy, require a hard link, or copy when hard links are unsupported.
    #[arg(long, value_enum, default_value = "hard-link")]
    mode: FileStrategy,
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
            mode: args.mode.into(),
            parallelism: args.parallelism,
        },
        |event| display.event(event),
    )
    .with_context(|| format!("failed to create backup {:?}", args.backup))?;
    display.finish(&outcome)
}
