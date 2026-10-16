//! Shared mod inspection and rendering, independent of file selection and loader origin.

use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry},
    modloader::{LoaderId, ModLoaderMeta},
};
use thiserror::Error;

use super::{ignored_output, loaders, source_path::PathArgs};

mod deps;
mod info;
mod input;

pub(in crate::commands) use input::FileSelectionArgs;

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

/// Identifies files with the caller's loader set, preserving successful records
/// when another loader or file fails. Input paths are already resolved by callers.
pub(in crate::commands) fn run(
    paths: impl IntoIterator<Item = PathBuf>,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    display: Display<'_>,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let mut failed = 0;
    let mut ignored = 0;
    let mut written = false;
    for path in paths {
        let mut failed_loaders = Vec::new();
        let result = ModEntry::new(path.clone())
            .context("failed to validate mod file path")
            .and_then(|entry| {
                mcmod::open(entry, loaders.clone(), |_, loader, _, error| {
                    crate::diagnostics::error(error);
                    failed_loaders.push(loader.id().as_str().to_owned());
                    IdentificationErrorAction::Skip
                })
            });
        let mut file = match result {
            Ok(file) => file,
            Err(error) => {
                crate::diagnostics::error(&error.context(format!("failed to inspect {path:?}")));
                failed += 1;
                continue;
            }
        };
        loaders::sort_metadata(&mut file);
        if !failed_loaders.is_empty() {
            failed += 1;
        } else if file.metadata.is_empty() {
            if hide_unrecognized {
                ignored += 1;
            } else {
                crate::diagnostics::error(&InspectionError::UnrecognizedModFile { path }.into());
                failed += 1;
            }
            continue;
        }
        // Dependency output can display incomplete identification even without
        // successful declarations; info only displays actual metadata records.
        if matches!(display, Display::Info) && file.metadata.is_empty() {
            continue;
        }
        if written {
            writeln!(stdout)
                .with_context(|| format!("failed to write {} separator", display.description()))?;
        }
        match display {
            Display::Info => info::write_info(&mut stdout, &file),
            Display::Dependencies(paths) => {
                deps::write_deps(&mut stdout, &file, &failed_loaders, paths)
            }
        }
        .with_context(|| format!("failed to write {}", display.description()))?;
        stdout
            .flush()
            .with_context(|| format!("failed to flush {}", display.description()))?;
        written = true;
    }
    if matches!(display, Display::Info) {
        ignored_output::write_loader_filtered(&mut stdout, ignored, &[])
            .context("failed to write ignored mod files summary")?;
        stdout.flush().context("failed to flush mod information")?;
    }
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
    if crate::terminal::is_interactive() {
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
    UnrecognizedModFile { path: PathBuf },
    #[error("failed to inspect {count} mod file(s)")]
    FailedInfo { count: usize },
    #[error("failed to inspect dependencies of {count} mod file(s)")]
    FailedDependencies { count: usize },
}
