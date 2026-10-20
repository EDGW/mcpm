//! Shared mod listing, identification diagnostics and terminal output.

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::ModEntry,
    modloader::{LoaderId, ModLoaderMeta},
};
use thiserror::Error;

use crate::terminal;

use super::{
    progress,
    scan::ScanSession,
    selection::{Resolver, Scope, SelectedFile, SelectionArgs},
    source_path::PathArgs,
};
use columns::ColumnArgs;

mod columns;
mod output;
mod rows;
mod tui;

#[derive(Args)]
pub(in crate::commands) struct ListDisplayArgs {
    /// Print a Console list instead of opening the TUI.
    #[arg(long)]
    no_ui: bool,
    /// Maximum simultaneous source lookups in TUI; ignored by Console and Raw.
    #[arg(long, default_value = "10", value_name = "COUNT")]
    identify_parallelism: std::num::NonZeroUsize,
    /// Show every mod declaration and list ignored filenames in formatted terminal output.
    #[arg(long)]
    expanded: bool,
    /// Index layout: full indices or aligned first items with compact nesting and indented names.
    #[arg(long, value_enum, default_value = "flat", value_name = "TYPE")]
    index_type: rows::IndexType,
    /// Override Console layout width; ignored by TUI, --raw or redirected output.
    #[arg(long, value_name = "WIDTH", value_parser = clap::value_parser!(u16).range(1..))]
    width: Option<u16>,
    #[command(flatten)]
    columns: ColumnArgs,
    #[command(flatten)]
    paths_output: PathArgs,
}

type ListedFile = SelectedFile;

/// Lists batches of entries supplied by a command, retaining per-input diagnostics.
/// Loader selection and input resolution belong to the caller.
pub(in crate::commands) fn run(
    inputs: impl IntoIterator<Item = Result<Vec<ModEntry>>>,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    args: ListDisplayArgs,
) -> Result<()> {
    let source_loaders = source_loaders(&loaders, hide_unrecognized);
    let progress = progress::Progress::new();
    let mut entries = Vec::new();
    let mut input_errors = 0;
    for input in inputs {
        match input {
            Ok(found) => entries.extend(found),
            Err(error) => {
                progress.report(&error);
                input_errors += 1;
            }
        }
    }
    progress.set_total(entries.len());

    let mut session = ScanSession::new(loaders);
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path().to_owned();
        let identified = session.read(path.clone(), Some(&progress));
        progress.advance();
        let Some(scanned) = identified else {
            continue;
        };
        files.push(SelectedFile {
            scanned,
            records: None,
        });
    }
    drop(progress);
    present(
        files,
        hide_unrecognized,
        args,
        input_errors,
        session.failed_count(),
        source_loaders,
    )
}

/// Resolves server list queries through the same lazy indexes as info/deps.
pub(in crate::commands) fn run_selection(
    selection: SelectionArgs,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    args: ListDisplayArgs,
) -> Result<()> {
    let source_loaders = source_loaders(&loaders, true);
    let mut resolver = Resolver::new(ScanSession::new(loaders), scope);
    let mut files = Vec::new();
    let mut input_errors = 0;
    let queries = match selection.into_queries() {
        Some(queries) if !queries.is_empty() => queries.into_iter().map(Some).collect(),
        _ => vec![None],
    };
    for query in queries {
        let result = match query {
            Some(query) => resolver.resolve_list(query),
            None => resolver.all(),
        };
        match result {
            Ok(selected) => files.extend(selected),
            Err(error) => {
                crate::diagnostics::error(&error);
                input_errors += 1;
            }
        }
    }
    present(
        files,
        true,
        args,
        input_errors,
        resolver.scan.failed_count(),
        source_loaders,
    )
}

/// Shared filtering, layout and summary for path-based and query-based listing.
fn present(
    files: Vec<ListedFile>,
    hide_unrecognized: bool,
    args: ListDisplayArgs,
    input_errors: usize,
    failed: usize,
    source_loaders: Vec<String>,
) -> Result<()> {
    let formatted = terminal::stdout_is_tty();
    let interactive = formatted && terminal::stderr_is_tty() && !args.no_ui;
    let mut ignored = 0;
    let mut ignored_files = Vec::new();
    let files: Vec<_> = files
        .into_iter()
        .filter(|listed| {
            let file = &listed.scanned.file;
            if hide_unrecognized
                && listed.scanned.failures.is_empty()
                && file.metadata.iter().all(|metadata| !metadata.has_mods())
            {
                ignored += 1;
                if formatted && (args.expanded || interactive) {
                    ignored_files.push(file.entry.path().to_owned());
                }
                false
            } else {
                true
            }
        })
        .collect();
    let columns = args.columns.selected();
    let color = terminal::stdout_color_enabled();
    let width = formatted.then(|| {
        args.width
            .map(usize::from)
            .unwrap_or_else(textwrap::termwidth)
    });
    let options = rows::RowOptions {
        expanded: args.expanded,
        paths: &args.paths_output,
        index_type: args.index_type,
    };
    if interactive {
        tui::run(
            &files,
            &columns,
            options,
            &source_loaders,
            args.identify_parallelism.get(),
            tui::Summary {
                ignored,
                ignored_files: &ignored_files,
                input_errors,
                failed,
            },
        )?;
    } else {
        let rows = rows::collect(&files, &options);
        let mut stdout = io::stdout().lock();
        output::write_list(&mut stdout, &rows, &columns, color, width)
            .context("failed to write mod list")?;
        super::ignored_output::write_loader_filtered(&mut stdout, ignored, &ignored_files)
            .context("failed to write ignored mod files summary")?;
        stdout.flush().context("failed to flush mod list")?;
    }

    if input_errors > 0 || failed > 0 {
        return Err(ListError::FailedListing {
            input_paths: input_errors,
            mod_files: failed,
        }
        .into());
    }
    Ok(())
}

/// Forward explicit CLI or server loader constraints, leaving an unfiltered list unrestricted.
fn source_loaders(loaders: &HashMap<LoaderId, ModLoaderMeta>, filtered: bool) -> Vec<String> {
    if !filtered {
        return Vec::new();
    }
    let mut ids: Vec<_> = loaders.keys().map(|id| id.as_str().to_owned()).collect();
    ids.sort();
    ids
}

#[derive(Debug, Error)]
enum ListError {
    #[error(
        "failed to list mods: {input_paths} input path error(s), {mod_files} failed mod file(s)"
    )]
    FailedListing {
        input_paths: usize,
        mod_files: usize,
    },
}
