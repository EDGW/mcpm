//! Resolves user-selected mod files for shared deps inspection.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, loaders::LoaderArgs, source_path::PathArgs};

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Remote: supported through mcpm remote (command ID: mod.deps) and --ssh.\n\nAdditional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: crate::commands::mod_support::selection::SelectionArgs,
    #[command(flatten)]
    stdin: crate::commands::mod_support::selection::StdinArgs,
    /// Directory searched by modid/name and --all; relative filenames use this directory.
    #[arg(long, value_name = "PATH", default_value = ".")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    loading: inspect::LoadingArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let selection = args.stdin.read(args.files)?.into_selection();
    super::inspection::run(
        selection,
        args.directory,
        args.loaders,
        args.loading.parallelism(),
        inspect::Display::Dependencies(&args.paths_output),
        ssh,
    )
}
