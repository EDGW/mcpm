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

#[derive(Args)]
pub(in crate::commands) struct ListDisplayArgs {
    /// Show every mod declaration and list ignored filenames in formatted terminal output.
    #[arg(long)]
    expanded: bool,
    /// Index layout: full indices or aligned first items with compact nesting and indented names.
    #[arg(long, value_enum, default_value = "flat", value_name = "TYPE")]
    index_type: output::IndexType,
    /// Override terminal layout width; ignored for --raw or redirected output.
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
    let formatted = terminal::is_interactive();
    let progress = progress::Progress::new(formatted);
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
    )
}

/// Resolves server list queries through the same lazy indexes as info/deps.
pub(in crate::commands) fn run_selection(
    selection: SelectionArgs,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    args: ListDisplayArgs,
) -> Result<()> {
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
    )
}

/// Shared filtering, layout and summary for path-based and query-based listing.
fn present(
    files: Vec<ListedFile>,
    hide_unrecognized: bool,
    args: ListDisplayArgs,
    input_errors: usize,
    failed: usize,
) -> Result<()> {
    let formatted = terminal::is_interactive();
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
                if args.expanded && formatted {
                    ignored_files.push(file.entry.path().to_owned());
                }
                false
            } else {
                true
            }
        })
        .collect();
    let columns = args.columns.selected();
    let color = terminal::color_enabled();
    let width = formatted.then(|| {
        args.width
            .map(usize::from)
            .unwrap_or_else(textwrap::termwidth)
    });
    let mut stdout = io::stdout().lock();
    output::write_list(
        &mut stdout,
        &files,
        output::ListOptions {
            expanded: args.expanded,
            color,
            columns: &columns,
            width,
            paths: &args.paths_output,
            index_type: args.index_type,
        },
    )
    .context("failed to write mod list")?;
    super::ignored_output::write_loader_filtered(&mut stdout, ignored, &ignored_files)
        .context("failed to write ignored mod files summary")?;
    stdout.flush().context("failed to flush mod list")?;

    if input_errors > 0 || failed > 0 {
        return Err(ListError::FailedListing {
            input_paths: input_errors,
            mod_files: failed,
        }
        .into());
    }
    Ok(())
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
