mod output;

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mcmod::{self, IdentificationErrorAction, ModEntry};
use thiserror::Error;

use super::loaders::LoaderArgs;

#[derive(Args)]
pub(super) struct DepsArgs {
    /// Paths to .jar or .jar.disabled mod files.
    #[arg(value_name = "FILE", required = true, num_args = 1..)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    loaders: LoaderArgs,
}

#[derive(Debug, Error)]
enum DepsError {
    #[error("no supported loader recognized mod file {path:?}")]
    UnrecognizedModFile { path: PathBuf },
    #[error("failed to inspect dependencies of {count} mod file(s)")]
    FailedFiles { count: usize },
}

pub(super) fn run(args: DepsArgs) -> Result<()> {
    let filtered = args.loaders.is_filtered();
    let loaders = args.loaders.select()?;
    let mut stdout = io::stdout().lock();
    let mut failed = 0;
    let mut written = false;

    for path in args.files {
        let mut failed_loaders = Vec::new();
        let result = ModEntry::new(path.clone())
            .context("failed to validate mod file path")
            .and_then(|entry| {
                mcmod::open(entry, loaders.clone(), |_, loader, _, error| {
                    crate::diagnostics::error(error);
                    failed_loaders.push(loader.id.clone());
                    IdentificationErrorAction::Skip
                })
            });
        let file = match result {
            Ok(file) => file,
            Err(error) => {
                crate::diagnostics::error(&error.context(format!("failed to inspect {path:?}")));
                failed += 1;
                continue;
            }
        };
        if !failed_loaders.is_empty() {
            failed += 1;
        } else if file.metadata.is_empty() && !filtered {
            crate::diagnostics::error(&DepsError::UnrecognizedModFile { path }.into());
            failed += 1;
            continue;
        }
        if file.metadata.is_empty() && failed_loaders.is_empty() {
            continue;
        }
        if written {
            writeln!(stdout).context("failed to write dependency separator")?;
        }
        output::write_deps(&mut stdout, &file, &failed_loaders)
            .context("failed to write dependency declarations")?;
        stdout
            .flush()
            .context("failed to flush dependency declarations")?;
        written = true;
    }

    if failed > 0 {
        return Err(DepsError::FailedFiles { count: failed }.into());
    }
    Ok(())
}
