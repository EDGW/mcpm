mod columns;
mod output;
mod progress;

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModDirectory, ModFile},
    modloader::ModLoaderMeta,
};
use thiserror::Error;

use super::loaders::LoaderArgs;
use columns::ColumnArgs;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Directory containing .jar and .jar.disabled files (not recursive).
    #[arg(value_name = "DIR", default_value = ".")]
    directory: PathBuf,
    /// Show every mod declaration instead of a summary for each file.
    #[arg(long)]
    expanded: bool,
    /// Print unaligned, unwrapped text without colors (also used when stdout is not a TTY).
    #[arg(long)]
    raw: bool,
    /// Override terminal layout width; ignored for --raw or redirected output.
    #[arg(long, value_name = "WIDTH", value_parser = clap::value_parser!(u16).range(1..))]
    width: Option<u16>,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    columns: ColumnArgs,
}

#[derive(Debug, Error)]
enum ListError {
    #[error("failed to identify {count} mod file(s)")]
    FailedFiles { count: usize },
}

struct ListedFile {
    file: ModFile,
    failures: Vec<FailedIdentification>,
}

struct FailedIdentification {
    loader: ModLoaderMeta,
    reason: String,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let formatted = io::stdout().is_terminal() && !args.raw;
    let progress = progress::Progress::new(formatted && io::stderr().is_terminal());
    let entries = ModDirectory::new(args.directory)?.entries()?;
    progress.set_total(entries.len());
    let filtered = args.loaders.is_filtered();
    let loaders = args.loaders.select()?;
    let columns = args.columns.selected();

    let mut files = Vec::new();
    let mut failed = 0;
    for entry in entries {
        let path = entry.path().to_owned();
        let mut failures = Vec::new();
        let identified = mcmod::open(entry, loaders.clone(), |_, loader, _, error| {
            progress.report(error);
            failures.push(FailedIdentification {
                loader: loader.clone(),
                reason: error.root_cause().to_string(),
            });
            IdentificationErrorAction::Skip
        });
        progress.advance();
        let file = match identified {
            Ok(file) => file,
            Err(error) => {
                progress.report(&error.context(format!("failed to identify {path:?}")));
                failed += 1;
                continue;
            }
        };
        if !failures.is_empty() {
            failed += 1;
        }
        if filtered
            && failures.is_empty()
            && file
                .metadata
                .iter()
                .all(|metadata| metadata.mods.is_empty())
        {
            continue;
        }
        files.push(ListedFile { file, failures });
    }
    drop(progress);
    let color = formatted && std::env::var_os("NO_COLOR").is_none();
    let width = formatted.then(|| {
        args.width
            .map(usize::from)
            .unwrap_or_else(textwrap::termwidth)
    });
    let mut stdout = io::stdout().lock();
    output::write_list(&mut stdout, &files, args.expanded, color, &columns, width)
        .context("failed to write mod list")?;
    stdout.flush().context("failed to flush mod list")?;

    if failed > 0 {
        return Err(ListError::FailedFiles { count: failed }.into());
    }
    Ok(())
}
