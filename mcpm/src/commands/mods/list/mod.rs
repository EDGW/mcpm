//! Lists user-supplied mod paths using the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use tracing::debug;

use crate::commands::stdin;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs, ListLoadingArgs},
    loaders::LoaderArgs,
};

mod input;
mod remote;
mod ssh;

#[derive(Args, Debug)]
#[command(
    defer(|command| crate::help::grouped(command, &[
        "Global options",
        "Mode options", "Loading options",
        "Filter",
        "Style options",
        "Console-only options",
    ])),
    after_help = "Modes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output.\n  Remote: supported through mcpm remote (command ID: mod.list); paths and loaders are structured parameters.\n\nTUI keys: Up/Down, PageUp/PageDown, Home/End; F opens Filter (@name, @modid, @file, @loaders; Esc/Q closes); Left/Right or Space expands/collapses one level; e expands all; Tab switches cards; c copies the selected Details field; Enter opens files for a matched source Details field, or toggles full details; i shows skipped files; q/Esc quits."
)]
pub(super) struct ListArgs {
    /// Mod files or directories; append paths from stdin, one per line. Defaults to . when empty.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loading: ListLoadingArgs,
    #[command(flatten, next_help_heading = "Filter")]
    loaders: LoaderArgs,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: ListArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let started = std::time::Instant::now();
    let _span = tracing::info_span!(
        "mod_list",
        source = if ssh.is_requested() { "ssh" } else { "local" }
    )
    .entered();
    tracing::info!("Starting mod list");
    debug!(loaders = ?args.loaders, loading = ?args.loading, display = ?args.display, "Selected mod list options");
    ssh.validate()?;
    let mut paths = stdin::append(args.paths)?;
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    debug!(?paths, "Resolved mod list input paths");
    let result = if ssh.is_requested() {
        debug!("Dispatching remote mod.list request over SSH");
        self::ssh::run(
            ssh,
            paths,
            args.loaders,
            args.loading.parallelism(),
            args.display,
        )
    } else {
        debug!("Dispatching local mod.list request locally");
        let inputs = paths.into_iter().map(|path| {
            let context = format!("failed to read list input {path:?}");
            input::read(path).context(context)
        });
        list::run(
            inputs,
            args.loaders.select(),
            args.loaders.is_filtered(),
            args.loading.parallelism(),
            args.display,
        )
    };
    tracing::info!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "Mod list ended"
    );
    result
}
