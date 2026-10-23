//! Server-scoped deps using shared mod inspection and presentation.

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::{inspect, source_path::PathArgs};

use super::input::FileArgs;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Remote supported through --ssh; server paths and selections resolve on the SSH host.\n\nAdditional inputs are read from redirected stdin, one per line; --input sets their interpretation (default: filename, modid, then name matching)."
)]
pub(super) struct DepsArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    loading: inspect::LoadingArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

pub(super) fn run(args: DepsArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    super::inspection::run(
        args.files,
        args.loading.parallelism(),
        inspect::Display::Dependencies(&args.paths_output),
        ssh,
    )
}
