//! Resolves ordered server mod queries and lists them through `mods.jar.read`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::mods::selection::SelectionSpec;
use mcpm_backend::mods::types::{ModsCachePolicy, ModsScope};
use mcpm_backend::mods::{JarRead, ReadEvent, ReadInput};

use super::super::target;
use crate::commands::execution;
use crate::commands::supports::mods::{
    list::{self, Counts, ListDisplayArgs, ListLoadingArgs},
    reuse::QueryArgs,
    scan_output::ScanOutput,
    selection::SelectionArgs,
};
use crate::help;
use crate::ssh::SshArgs;

#[derive(Args, Debug)]
#[command(
    defer(|command| help::grouped(command, &[
        "Global options", "Mode options", "Loading options", "Input options", "Filter", "Style options",
        "Console-only options",
    ])),
    after_help = "Remote supported through --ssh. Server paths and selections resolve on the SSH host; presentation and source provider requests stay local.\n\nAdditional inputs are read from redirected stdin, one per line, using filename, modid, then name matching. Paths are relative to the server's mods directory; mods/ prefixes are also accepted. With no inputs, list all mods.\n\nModes:\n  TUI (default in a terminal): browse, search and inspect mods; identify outer JARs on Modrinth and CurseForge (default parallelism: 10).\n  Console (--no-ui): print a formatted list.\n  Raw (--raw or redirected stdout): tab-separated output."
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
    let loading = args.loading.parallelism();
    let display = args.display;
    let selection = SelectionSpec::from_selection(args.selection.with_stdin()?.into_selection())?;
    let target = target::resolve(args.path, ssh)?;
    let remote = target.session.target().is_remote();
    let request = ReadInput {
        scope: ModsScope::Server {
            path: target.path.clone(),
        },
        selection,
        parallelism: loading,
        cache: if args.cache.no_cache {
            ModsCachePolicy::Disabled
        } else {
            ModsCachePolicy::ScopeDefault
        },
    };
    let runtime = execution::Runtime::new()?;
    let (files, counts, source_loaders) = runtime.block_on(async {
        let mut handle = target
            .session
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
    let cache = if args.cache.no_cache {
        None
    } else {
        Some(mcpm_backend::data::cache::mods::ModsCacheSelection::Default)
    };
    let identify = crate::flows::identify::Target {
        session: target.session.clone(),
        cache,
    };
    let result = list::present(files, true, display, counts, source_loaders, identify);
    execution::finish(&runtime, &target.session)?;
    result
}
