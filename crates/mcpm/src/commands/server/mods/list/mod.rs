//! Resolves ordered server mod queries for the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::{
    commands::supports::mods::{
        list::{self, ListDisplayArgs, ListLoadingArgs},
        reuse::QueryArgs,
        selection::SelectionArgs,
    },
    help,
    ssh::SshArgs,
};

#[derive(Args, Debug)]
#[command(
    defer(|command| help::grouped(command, &[
        "Global options", "Mode options", "Loading options", "Input options", "Filter", "Style options",
        "Console-only options",
    ])),
    after_help = "Remote supported through --ssh. Server paths, selections and source provider requests belong to the SSH host; presentation stays local.\n\nAdditional inputs are read from redirected stdin, one per line, using filename, modid, then name matching. Paths are relative to the server's mods directory; mods/ prefixes are also accepted. With no inputs, list all mods.\n\nModes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output."
)]
pub(super) struct ListArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(
        long = "server",
        value_name = "PATH",
        default_value = ".",
        help_heading = "Input options"
    )]
    path: PathBuf,
    #[command(flatten, next_help_heading = "Input options")]
    selection: SelectionArgs,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loading: ListLoadingArgs,
    #[command(flatten)]
    cache: QueryArgs,
}

pub(super) fn run(args: ListArgs, ssh: SshArgs) -> Result<()> {
    let selection = args.selection.with_stdin()?.into_selection();
    list::run(
        mcpm_backend::operations::mods::list::Input {
            scope: mcpm_backend::operations::mods::loading::ScopeInput::Server {
                path: args.path,
                no_cache: args.cache.no_cache,
            },
            selection: mcpm_backend::operations::mods::list::Selection::Queries(
                mcpm_backend::operations::mods::loading::selection::SelectionInput::from_selection(
                    selection,
                )?,
            ),
            parallelism: std::num::NonZeroUsize::new(args.loading.parallelism()).unwrap(),
        },
        args.display,
        ssh,
    )
}
