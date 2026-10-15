mod columns;
mod input;
mod output;
mod progress;

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModFile},
    modloader::ModLoaderMeta,
};
use thiserror::Error;

use crate::terminal::{self, Stream};

use super::loaders::LoaderArgs;
use columns::ColumnArgs;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Mod files or directories, processed in argument order (directories are not recursive).
    #[arg(value_name = "PATH", default_value = ".", num_args = 1..)]
    paths: Vec<PathBuf>,
    /// Show every mod declaration and list ignored filenames in formatted terminal output.
    #[arg(long)]
    expanded: bool,
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
    #[error(
        "failed to list mods: {input_paths} input path error(s), {mod_files} failed mod file(s)"
    )]
    FailedListing {
        input_paths: usize,
        mod_files: usize,
    },
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
    let formatted = terminal::is_tty(Stream::Stdout);
    let progress = progress::Progress::new(formatted && terminal::is_tty(Stream::Stderr));
    let mut entries = Vec::new();
    let mut input_errors = 0;
    for path in args.paths {
        let context = format!("failed to read list input {path:?}");
        match input::read(path).context(context) {
            Ok(found) => entries.extend(found),
            Err(error) => {
                progress.report(&error);
                input_errors += 1;
            }
        }
    }
    progress.set_total(entries.len());
    let filtered = args.loaders.is_filtered();
    let loaders = args.loaders.select()?;
    let columns = args.columns.selected();

    let mut files = Vec::new();
    let mut failed = 0;
    let mut ignored = 0;
    let mut ignored_files = Vec::new();
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
            && file.metadata.iter().all(|metadata| !metadata.has_mods())
        {
            ignored += 1;
            if args.expanded && formatted {
                ignored_files.push(path);
            }
            continue;
        }
        files.push(ListedFile { file, failures });
    }
    drop(progress);
    let color = terminal::color_enabled(Stream::Stdout);
    let width = formatted.then(|| {
        args.width
            .map(usize::from)
            .unwrap_or_else(textwrap::termwidth)
    });
    let mut stdout = io::stdout().lock();
    output::write_list(&mut stdout, &files, args.expanded, color, &columns, width)
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
