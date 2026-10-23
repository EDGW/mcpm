//! Explicit cache management commands; disk operations belong to kako-mods-lib.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Result, ensure};
use clap::{Args, Subcommand};
use kako_mods_lib::local::mcmod::cache::{
    CacheId, CacheStatus, MetadataRefresh, ModsCache, VerificationMode,
};

use super::{cache_support::FileStrategy, mod_support::loaders::LoaderArgs};

mod output;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout; one JSON object per line).\n\nCache management is local and explicit. Mod info/list query existing metadata by default; --cache-dir selects another cache and --no-cache disables reuse; downloads do not use it automatically."
)]
pub(crate) struct CacheArgs {
    #[command(subcommand)]
    command: CacheCommand,
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Manage cached mod files and their metadata (Console, Raw).
    Mods(ModsArgs),
}

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout; one JSON object per line).\n\nDefault directory: ~/.mcpm/cache/mods. Import defaults to copying; --mode hard-link also protects the source against content writes. Imports deduplicate by BLAKE3 and do not identify mods.\nRefresh explicitly identifies cached bytes; --loader/--loaders select loaders, otherwise all built-ins are used.\nExports never overwrite or create parent directories. Hard links share content write protection; update linked files by replacing them.\nRemove deletes only the cache names and does not inspect backup references. No automatic eviction."
)]
struct ModsArgs {
    /// Cache root directory; defaults to ~/.mcpm/cache/mods.
    #[arg(long, global = true, value_name = "PATH")]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: ModsCommand,
}

#[derive(Debug, Subcommand)]
enum ModsCommand {
    /// Initialize an absent or empty cache directory (Console, Raw).
    Init,
    /// Import or reuse content-addressed objects without parsing them (Console, Raw).
    #[command(
        after_help = "Files are processed in input order. The first failure stops processing; previously imported objects remain. Each successful file produces one output record."
    )]
    Import {
        #[arg(required = true, num_args = 1.., value_name = "PATH")]
        paths: Vec<PathBuf>,
        /// Copy, require a hard link, or allow copying on filesystem capability failures.
        /// Hard links also protect the source against content writes.
        #[arg(long, value_enum, default_value = "copy")]
        mode: FileStrategy,
    },
    /// List stored objects, including incomplete or invalid objects (Console, Raw).
    List,
    /// Rebuild the optional imohash lookup index from stored objects (Console, Raw).
    RebuildIndex,
    /// Show file details and cached mod information (Console, Raw).
    Info {
        #[arg(value_name = "BLAKE3")]
        id: CacheId,
    },
    /// Identify cached bytes and atomically update selected loader results (Console, Raw).
    Refresh {
        #[arg(value_name = "BLAKE3")]
        id: CacheId,
        #[command(flatten)]
        loaders: LoaderArgs,
        /// Reparse selected loaders even when current results exist.
        #[arg(long)]
        force: bool,
    },
    /// Check file contents against the BLAKE3 ID and saved digest (Console, Raw).
    Verify {
        #[arg(value_name = "BLAKE3")]
        id: CacheId,
        /// Check structure and size without computing full content hashes.
        #[arg(long)]
        structure_only: bool,
    },
    /// Export one object's bytes to multiple destinations (Console, Raw).
    #[command(
        after_help = "Usage: export BLAKE3 PATH...\nDestinations are processed in input order. The first failure stops processing; previously exported files remain. Each successful destination produces one output record.\nFiles without a digest can be exported; contents are checked against the BLAKE3 ID before publication."
    )]
    Export {
        #[arg(value_name = "BLAKE3")]
        id: CacheId,
        #[arg(required = true, num_args = 1.., value_name = "PATH")]
        destinations: Vec<PathBuf>,
        /// Copy, require a hard link, or allow copying on filesystem capability failures.
        #[arg(long, value_enum, default_value = "copy")]
        mode: FileStrategy,
    },
    /// Delete an object's cache names; exported hard links remain (Console, Raw).
    Remove {
        #[arg(value_name = "BLAKE3")]
        id: CacheId,
    },
}

pub(crate) fn run(args: CacheArgs) -> Result<()> {
    let CacheCommand::Mods(args) = args.command;
    let cache = match args.directory {
        Some(path) => ModsCache::new(path),
        None => ModsCache::user_default()?,
    };
    let mut stdout = io::stdout().lock();
    match args.command {
        ModsCommand::Init => {
            cache.initialize()?;
            output::initialized(&mut stdout, cache.root())?;
        }
        ModsCommand::Import { paths, mode } => {
            let mut writer = cache.writer()?;
            for path in paths {
                let imported = writer.import_with_mode(&path, mode.into())?;
                output::imported(&mut stdout, &writer.get(imported.id), imported)?;
            }
        }
        ModsCommand::List => {
            for entry in cache.reader()?.list()? {
                output::entry(&mut stdout, &entry)?;
            }
        }
        ModsCommand::RebuildIndex => {
            output::rebuilt_index(&mut stdout, cache.writer()?.rebuild_index()?)?;
        }
        ModsCommand::Info { id } => {
            let reader = cache.reader()?;
            let entry = reader.get(id);
            output::info(&mut stdout, &entry)?;
        }
        ModsCommand::Refresh { id, loaders, force } => {
            let mode = if force {
                MetadataRefresh::Force
            } else {
                MetadataRefresh::MissingOrOutdated
            };
            output::entry(
                &mut stdout,
                &cache
                    .writer()?
                    .refresh_metadata(id, &loaders.select(), mode)?,
            )?;
        }
        ModsCommand::Verify { id, structure_only } => {
            let mode = if structure_only {
                VerificationMode::Structure
            } else {
                VerificationMode::Content
            };
            let status = cache.reader()?.verify(id, mode);
            output::verification(&mut stdout, id, &status)?;
            ensure!(
                status == CacheStatus::Available,
                "cache verification failed for {id}: {status:?}"
            );
        }
        ModsCommand::Export {
            id,
            destinations,
            mode,
        } => {
            let reader = cache.reader()?;
            for destination in destinations {
                let method = reader.export(id, &destination, mode.into())?;
                output::exported(&mut stdout, id, &destination, method)?;
            }
        }
        ModsCommand::Remove { id } => {
            output::removed(&mut stdout, id, cache.writer()?.remove(id)?)?;
        }
    }
    stdout.flush()?;
    Ok(())
}
