//! Shared mod inspection and rendering, independent of file selection and loader origin.

use std::{
    collections::HashMap,
    io::{self, Write},
};

use anyhow::{Context, Result};
use kako_mods_lib::local::modloader::{LoaderId, ModLoaderMeta};
use thiserror::Error;

use super::{
    ignored_output,
    scan::ScanSession,
    selection::{Resolver, Scope, SelectedFile},
    source_path::PathArgs,
};

mod deps;
mod info;

pub(in crate::commands) use super::selection::SelectionArgs;

/// Selects the presentation without changing identification or input semantics.
#[derive(Clone, Copy)]
pub(in crate::commands) enum Display<'a> {
    Info,
    Dependencies(&'a PathArgs),
}

impl Display<'_> {
    fn description(self) -> &'static str {
        match self {
            Self::Info => "mod information",
            Self::Dependencies(_) => "dependency declarations",
        }
    }
}

/// Resolves ordered queries and renders selected files or archive subtrees.
pub(in crate::commands) fn run(
    selection: SelectionArgs,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    display: Display<'_>,
) -> Result<()> {
    let mut resolver = Resolver::new(ScanSession::new(loaders), scope);
    let mut stdout = io::stdout().lock();
    let mut selection_failures = 0;
    let mut ignored = 0;
    let mut written = false;
    let queries = match selection.into_queries() {
        Some(queries) => queries.into_iter().map(Some).collect(),
        None => vec![None],
    };
    for query in queries {
        let result = match query {
            Some(query) => resolver.resolve(query),
            None => resolver.all(),
        };
        let selected = match result {
            Ok(selected) => selected,
            Err(error) => {
                crate::diagnostics::error(&error);
                selection_failures += 1;
                continue;
            }
        };
        for SelectedFile { scanned, records } in selected {
            let file = &scanned.file;
            if scanned.failures.is_empty() && file.metadata.is_empty() {
                if hide_unrecognized {
                    ignored += 1;
                } else {
                    crate::diagnostics::error(
                        &InspectionError::UnrecognizedModFile {
                            path: file.entry.path().to_owned(),
                        }
                        .into(),
                    );
                    selection_failures += 1;
                }
                continue;
            }
            if matches!(display, Display::Info) && file.metadata.is_empty() {
                continue;
            }
            if written {
                writeln!(stdout).with_context(|| {
                    format!("failed to write {} separator", display.description())
                })?;
            }
            match display {
                Display::Info => info::write_info(&mut stdout, file, records.as_deref()),
                Display::Dependencies(paths) => deps::write_deps(
                    &mut stdout,
                    file,
                    &scanned
                        .failures
                        .iter()
                        .map(|failure| failure.loader.id().as_str().to_owned())
                        .collect::<Vec<_>>(),
                    paths,
                    records.as_deref(),
                ),
            }
            .with_context(|| format!("failed to write {}", display.description()))?;
            stdout
                .flush()
                .with_context(|| format!("failed to flush {}", display.description()))?;
            written = true;
        }
    }
    if matches!(display, Display::Info) {
        ignored_output::write_loader_filtered(&mut stdout, ignored, &[])
            .context("failed to write ignored mod files summary")?;
        stdout.flush().context("failed to flush mod information")?;
    }
    let failed = resolver.scan.failed_count() + selection_failures;
    if failed > 0 {
        return Err(match display {
            Display::Info => InspectionError::FailedInfo { count: failed },
            Display::Dependencies(_) => InspectionError::FailedDependencies { count: failed },
        }
        .into());
    }
    Ok(())
}

/// Empty selection succeeds without scanning; interactive users receive guidance.
pub(in crate::commands) fn prompt_for_input() -> Result<()> {
    if crate::terminal::stderr_is_tty() {
        writeln!(
            io::stderr().lock(),
            "Specify at least one file, or use --all to inspect all mods."
        )
        .context("failed to write empty input hint")?;
    }
    Ok(())
}

#[derive(Debug, Error)]
enum InspectionError {
    #[error("no supported loader recognized mod file {path:?}")]
    UnrecognizedModFile { path: std::path::PathBuf },
    #[error("mod inspection completed with {count} input or identification failure(s)")]
    FailedInfo { count: usize },
    #[error("dependency inspection completed with {count} input or identification failure(s)")]
    FailedDependencies { count: usize },
}
