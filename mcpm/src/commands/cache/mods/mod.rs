//! Mod cache command selection and shared directory/Raw header options.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_backend::local::cache::mods::ModsCacheSelection;

mod batch;
mod columns;
mod entry;
mod export;
mod import;
mod info;
mod init;
mod input;
mod list;
mod rebuild_index;
mod refresh;
mod remove;
mod verify;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Raw list, info and simple operations use TSV; refresh succeeds silently.\n\nDefault directory: ~/.mcpm/cache/mods. Import stores content only; refresh populates the named meta or ident metadata. Verify checks content only. Exports never overwrite or create parent directories. No automatic eviction. All operations are local."
)]
pub(super) struct ModsArgs {
    /// Cache root; explicit directories must already be initialized.
    #[arg(long, global = true, value_name = "PATH")]
    directory: Option<PathBuf>,
    /// Include a header in Raw TSV output; ignored by Console and refresh.
    #[arg(long, global = true)]
    headers: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a missing or empty cache directory (Console, Raw).
    Init,
    /// Import content without calculating meta or ident (Console, Raw).
    Import(import::ImportArgs),
    /// List BLAKE3 IDs or selected cache columns (Console, Raw).
    List(list::ListArgs),
    /// Inspect cached content and named metadata (Console, Raw).
    Info(info::InfoArgs),
    /// Refresh meta or ident metadata (Console, Raw).
    Refresh(refresh::RefreshArgs),
    /// Verify cached content without reading metadata (Console, Raw).
    Verify(verify::VerifyArgs),
    /// Export one object's bytes to multiple destinations (Console, Raw).
    Export(export::ExportArgs),
    /// Remove selected or all cache-owned names; exported hard links remain (Console, Raw).
    Remove(remove::RemoveArgs),
    /// Rebuild the optional imohash lookup index (Console, Raw).
    RebuildIndex,
}

pub(super) fn run(args: ModsArgs) -> Result<()> {
    let selection = match args.directory {
        Some(path) => ModsCacheSelection::Directory { path },
        None => ModsCacheSelection::Default,
    };
    let mut out = io::stdout().lock();
    match args.command {
        Command::Init => init::run(selection, &mut out, args.headers)?,
        Command::Import(args_) => import::run(args_, selection, &mut out, args.headers)?,
        Command::List(args_) => list::run(args_, selection, &mut out, args.headers)?,
        Command::Info(args_) => info::run(args_, selection, &mut out, args.headers)?,
        Command::Refresh(args_) => refresh::run(args_, selection, &mut out)?,
        Command::Verify(args_) => verify::run(args_, selection, &mut out, args.headers)?,
        Command::Export(args_) => export::run(args_, selection, &mut out, args.headers)?,
        Command::Remove(args_) => remove::run(args_, selection, &mut out, args.headers)?,
        Command::RebuildIndex => rebuild_index::run(selection, &mut out, args.headers)?,
    }
    out.flush()?;
    Ok(())
}
