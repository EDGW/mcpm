mod output;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry, ModFile},
    modloader::ModLoaderSet,
};
use thiserror::Error;

use super::loaders::LoaderArgs;

#[derive(Args)]
pub(super) struct InfoArgs {
    /// Paths to .jar or .jar.disabled mod files.
    #[arg(value_name = "FILE", required = true, num_args = 1..)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    loaders: LoaderArgs,
}

#[derive(Debug, Error)]
enum InfoError {
    #[error("no supported loader recognized mod file {path:?}")]
    UnrecognizedModFile { path: PathBuf },
    #[error("failed to inspect {count} mod file(s)")]
    FailedFiles { count: usize },
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let filtered = args.loaders.is_filtered();
    let loaders = args.loaders.select()?;
    let mut stdout = io::stdout().lock();
    let mut failed = 0;
    let mut written = false;

    for path in args.files {
        let mut had_error = false;
        let file = match read_file(&path, &loaders, &mut had_error, filtered) {
            Ok(file) => file,
            Err(error) => {
                crate::diagnostics::error(&error.context(format!("failed to inspect {path:?}")));
                failed += 1;
                continue;
            }
        };
        if had_error {
            failed += 1;
        }
        if file.metadata.is_empty() {
            continue;
        }
        if written {
            writeln!(stdout).context("failed to write mod information separator")?;
        }
        output::write_info(&mut stdout, &file).context("failed to write mod information")?;
        stdout.flush().context("failed to flush mod information")?;
        written = true;
    }

    if failed > 0 {
        return Err(InfoError::FailedFiles { count: failed }.into());
    }
    Ok(())
}

fn read_file(
    path: &Path,
    loaders: &ModLoaderSet,
    had_error: &mut bool,
    filtered: bool,
) -> Result<ModFile> {
    let entry = ModEntry::new(path.to_owned()).context("failed to validate mod file path")?;
    let file = mcmod::open(entry, loaders.clone(), |_, _, _, error| {
        crate::diagnostics::error(error);
        *had_error = true;
        IdentificationErrorAction::Skip
    })?;
    if file.metadata.is_empty() && !*had_error && !filtered {
        return Err(InfoError::UnrecognizedModFile {
            path: file.entry.path().to_owned(),
        }
        .into());
    }

    Ok(file)
}
