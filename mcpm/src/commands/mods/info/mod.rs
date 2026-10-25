//! Resolves user-selected mod files for shared info inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::supports::mods::{inspect, loaders::LoaderArgs};

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Remote: supported through mcpm remote (command ID: mod.info) and --ssh.\n\nAdditional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct InfoArgs {
    #[command(flatten)]
    items: crate::commands::supports::mods::selection::ItemArgs,
    /// Directory searched by modid/name and --all; relative filenames use this directory.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    loading: inspect::LoadingArgs,
    #[command(flatten)]
    cache: crate::commands::supports::mods::reuse::CacheArgs,
}

pub(super) fn run(args: InfoArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let selection = args.items.read()?;
    super::inspection::run(
        selection,
        args.directory,
        args.loaders,
        &args.cache.environment(),
        crate::commands::supports::mods::scan::Options {
            parallelism: args.loading.parallelism(),
        },
        inspect::Display::Info,
        ssh,
    )
}
