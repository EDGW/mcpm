//! Lists user-supplied mod paths through the shared `mods.jar.read` operation.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::mods::types::{ModsCachePolicy, ModsScope};
use mcpm_backend::mods::{JarRead, ReadEvent, ReadInput};

use crate::commands::{execution, stdin};

use crate::commands::supports::mods::{
    list::{self, Counts, ListDisplayArgs, ListLoadingArgs},
    loaders::LoaderArgs,
    scan_output::ScanOutput,
};

#[derive(Args, Debug)]
#[command(
    defer(|command| crate::help::grouped(command, &[
        "Global options", "Mode options", "Loading options", "Input options", "Filter", "Style options",
        "Console-only options",
    ])),
    after_help = format!("Modes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output.\n  Remote: supported through --ssh; paths and loaders are structured parameters.\n\nTUI keys: Up/Down, PageUp/PageDown, Home/End; F opens Filter (@name, @modid, @file, @loaders; Esc/Q closes); Left/Right or Space expands/collapses one level; e expands all; Tab switches cards; c copies the selected Details field; Enter opens files for a matched source Details field, or toggles full details; i shows skipped files; q/Esc quits.")
)]
pub(super) struct ListArgs {
    /// Mod files or directories; append paths from stdin, one per line. Defaults to . when empty.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loading: ListLoadingArgs,
    #[command(flatten)]
    cache: crate::commands::supports::mods::reuse::CacheArgs,
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
    ssh.validate()?;
    let mut paths = stdin::append(args.paths)?;
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let environment = args.cache.environment();
    let cache_selection =
        crate::commands::supports::mods::reuse::selection(&environment);
    let loading = args.loading.parallelism();
    let display = args.display;
    let session = execution::session_for(&ssh)?;
    let remote = session.target().is_remote();
    let hide_unrecognized = args.loaders.is_filtered();
    let loaders = args.loaders.ids();
    let request = ReadInput {
        scope: ModsScope::Paths {
            directory: PathBuf::from("."),
            paths,
            loaders,
        },
        selection: Default::default(),
        parallelism: loading,
        cache: match cache_selection {
            Some(selection) => ModsCachePolicy::Select { selection },
            None => ModsCachePolicy::Disabled,
        },
    };
    let runtime = execution::Runtime::new()?;
    let (files, counts, source_loaders) = runtime.block_on(async {
        let mut handle = session
            .start::<JarRead>(request)
            .await
            .map_err(|failure| execution::failure(failure, remote))?;
        let output = ScanOutput::default();
        let mut records = Vec::new();
        let mut source_loaders = Vec::new();
        while let Some(event) = handle.next().await {
            match event {
                ReadEvent::Scope { loaders, .. } => source_loaders = loaders,
                ReadEvent::Scan { completed, total } => {
                    output.event(crate::commands::supports::mods::scan::Event::Progress {
                        completed,
                        total,
                    })?;
                }
                ReadEvent::Diagnostic { error } => {
                    output.event(
                        crate::commands::supports::mods::scan::Event::Diagnostic { error },
                    )?;
                }
                ReadEvent::Item { record, .. } => records.push(record),
            }
        }
        let summary = handle
            .finish()
            .await
            .map_err(|failure| execution::failure(failure, remote))?;
        let (files, unreadable) = list::files_from_records(records)
            .context("failed to rebuild listed mod records")?;
        let counts = Counts {
            input_errors: summary.input_errors + unreadable,
            failed: summary.failed,
            cache: summary.cache,
        };
        Ok::<_, anyhow::Error>((files, counts, source_loaders))
    })?;
    let target = crate::flows::identify::Target {
        session: session.clone(),
        cache: crate::commands::supports::mods::reuse::selection(&environment),
    };
    let result = list::present(
        files,
        hide_unrecognized,
        display,
        counts,
        source_loaders,
        target,
    );
    execution::finish(&runtime, &session)?;
    tracing::debug!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "Mod list ended"
    );
    result
}
