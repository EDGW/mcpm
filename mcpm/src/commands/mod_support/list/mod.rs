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

use crate::commands::{
    execution::DEFAULT_PARALLELISM, progress, source_support::identify::Location,
};
use crate::terminal;

use super::{
    scan::ScanSession,
    scan_output::ScanOutput,
    selection::{ResolveError, Resolver, Scope, SelectedFile, Selection},
    source_path::PathArgs,
};
use columns::ColumnArgs;
use filter::Filter;

mod columns;
mod filter;
mod output;
mod rows;
mod tui;

#[derive(Args, Debug)]
pub(in crate::commands) struct ListDisplayArgs {
    /// Filter matching rows with the same expression used by the TUI Filter card.
    #[arg(short = 'F', long, value_name = "FILTER", help_heading = "Filter", long_help = filter::HELP)]
    filter: Option<Filter>,
    /// Print a Console list instead of opening the TUI.
    #[arg(long, help_heading = "Mode options")]
    no_ui: bool,
    /// TUI only: maximum simultaneous source lookups; ignored by Console and Raw.
    #[arg(
        short = 'P',
        long,
        default_value_t = std::num::NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap(),
        value_name = "COUNT",
        help_heading = "Loading options"
    )]
    identify_parallelism: std::num::NonZeroUsize,
    /// Expand all branches in TUI; print every mod declaration in Console and Raw. Console also lists ignored filenames.
    #[arg(short = 'E', long, help_heading = "Style options")]
    expanded: bool,
    /// Index layout: full indices or aligned first items with compact nesting and indented names.
    #[arg(
        long,
        value_enum,
        default_value = "flat",
        value_name = "TYPE",
        help_heading = "Style options"
    )]
    index_type: rows::IndexType,
    /// Override Console layout width; ignored by TUI, --raw or redirected output.
    #[arg(long, value_name = "WIDTH", value_parser = clap::value_parser!(u16).range(1..), help_heading = "Console-only options")]
    width: Option<u16>,
    #[command(flatten, next_help_heading = "Style options")]
    columns: ColumnArgs,
    #[command(flatten, next_help_heading = "Style options")]
    paths_output: PathArgs,
}

/// File-host execution options are separate from local presentation options.
#[derive(Args, Debug)]
pub(in crate::commands) struct ListLoadingArgs {
    /// Maximum simultaneous outer-JAR inspections, locally or on the SSH host.
    #[arg(short = 'p', long, default_value_t = std::num::NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap(), value_name = "COUNT", help_heading = "Loading options")]
    parallelism: std::num::NonZeroUsize,
}
impl ListLoadingArgs {
    pub(in crate::commands) fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
}

type ListedFile = SelectedFile;

/// Lists batches of entries supplied by a command, retaining per-input diagnostics.
/// Loader selection and input resolution belong to the caller.
pub(in crate::commands) fn run(
    inputs: impl IntoIterator<Item = Result<Vec<ModEntry>>>,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    parallelism: usize,
    args: ListDisplayArgs,
) -> Result<()> {
    let started = std::time::Instant::now();
    tracing::debug!(loaders = ?loaders.keys().map(|id| id.as_str()).collect::<Vec<_>>(), hide_unrecognized, "Starting local mod list scan");
    let source_loaders = source_loaders(&loaders, hide_unrecognized);
    let progress = progress::Progress::new("Scanning mod directory...", "Identifying mods");
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
    tracing::debug!(
        count = entries.len(),
        input_errors,
        "Local mod list input discovery completed"
    );

    drop(progress);
    let output = ScanOutput::default();
    let observe = |event| output.event(event);
    let mut session = ScanSession::new(loaders)
        .with_parallelism(parallelism)
        .with_observer(&observe);
    let paths: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.path().to_owned())
        .collect();
    let files = session
        .collect(&paths)?
        .into_iter()
        .map(|scanned| SelectedFile {
            scanned,
            records: None,
        })
        .collect::<Vec<_>>();
    tracing::debug!(
        files = files.len(),
        failed = session.failed_count(),
        input_errors,
        elapsed_ms = started.elapsed().as_millis(),
        "Local mod list loading completed"
    );
    present(
        files,
        hide_unrecognized,
        args,
        input_errors,
        session.failed_count(),
        source_loaders,
        Location::Local,
    )
}

/// Resolves server list queries through the same lazy indexes as info/deps.
pub(in crate::commands) fn run_selection(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    parallelism: usize,
    args: ListDisplayArgs,
) -> Result<()> {
    let source_loaders = source_loaders(&loaders, true);
    let output = ScanOutput::default();
    let observe = |event| output.event(event);
    let mut resolver = Resolver::new(
        ScanSession::new(loaders)
            .with_parallelism(parallelism)
            .with_observer(&observe),
        scope,
    );
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
            Err(ResolveError::Execution(error)) => return Err(error),
            Err(ResolveError::Input(error)) => {
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
        Location::Local,
    )
}

/// Shared filtering, layout and summary for path-based and query-based listing.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn present(
    files: Vec<ListedFile>,
    hide_unrecognized: bool,
    args: ListDisplayArgs,
    input_errors: usize,
    failed: usize,
    source_loaders: Vec<String>,
    location: Location<'_>,
) -> Result<()> {
    let local_files = location.is_local();
    let formatted = terminal::stdout_is_tty();
    let interactive = formatted && terminal::stderr_is_tty() && !args.no_ui;
    tracing::debug!(
        mode = if interactive {
            "TUI"
        } else if formatted {
            "Console"
        } else {
            "Raw"
        },
        local_files,
        files = files.len(),
        failed,
        input_errors,
        "Preparing mod list presentation"
    );
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
                tracing::debug!(path = %file.entry.path().display(), "Hiding unrecognized mod file");
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
    tracing::debug!(retained = files.len(), ignored, filter = ?args.filter.as_ref().map(Filter::as_str), "Applied mod list visibility constraints");
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
    tracing::debug!(?columns, ?width, color, expanded = args.expanded, index_type = ?args.index_type, "Selected mod list layout");
    let presented = std::time::Instant::now();
    if interactive {
        tracing::info!(files = files.len(), "Opening mod list TUI");
        tui::run(
            &files,
            &columns,
            options,
            &source_loaders,
            args.identify_parallelism.get(),
            args.filter.unwrap_or_default(),
            tui::Summary {
                location,
                ignored,
                ignored_files: &ignored_files,
                input_errors,
                failed,
            },
        )?;
    } else {
        let rows = if let Some(filter) = args
            .filter
            .as_ref()
            .filter(|filter| !filter.as_str().trim().is_empty())
        {
            // Search every declaration before applying the compact/expanded view.
            let settings = rows::RowOptions {
                expanded: true,
                ..options
            };
            let mut rows: Vec<_> = files
                .iter()
                .enumerate()
                .flat_map(|(index, file)| {
                    let mut matches = rows::for_file(file, index + 1, &settings)
                        .into_iter()
                        .filter(|row| filter.matches(row));
                    if options.expanded {
                        matches.collect::<Vec<_>>()
                    } else {
                        matches.next().into_iter().collect()
                    }
                })
                .collect();
            rows::align(&mut rows);
            rows
        } else {
            rows::collect(&files, &options)
        };
        tracing::debug!(
            rows = rows.len(),
            files = files.len(),
            "Built and filtered mod list rows"
        );
        let mut stdout = io::stdout().lock();
        output::write_list(&mut stdout, &rows, &columns, color, width)
            .context("failed to write mod list")?;
        super::ignored_output::write_loader_filtered(&mut stdout, ignored, &ignored_files)
            .context("failed to write ignored mod files summary")?;
        stdout.flush().context("failed to flush mod list")?;
    }

    tracing::debug!(
        files = files.len(),
        ignored,
        failed,
        input_errors,
        elapsed_ms = presented.elapsed().as_millis(),
        "Mod list presentation completed"
    );
    if input_errors > 0 || failed > 0 {
        let error = ListError::FailedListing {
            input_paths: input_errors,
            mod_files: failed,
        }
        .into();
        return Err(if local_files {
            error
        } else {
            crate::diagnostics::RemoteError::wrap(error)
        });
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
