mod filter;
mod output;

use std::{
    collections::HashMap,
    fs::File,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mcmod::dependency::graph::{identifier::DependencyIdentifier, storage},
    modloader::BUILTIN_LOADERS,
};
use thiserror::Error;

use super::selector::ModSelector;

#[derive(Args)]
pub(super) struct ViewArgs {
    /// Graph JSON input; omit or use - to read stdin.
    #[arg(short, long, value_name = "FILE")]
    input: Option<PathBuf>,
    /// Root node, written as loader:modid.
    #[arg(value_name = "LOADER:MODID")]
    target: ModSelector,
    /// Show mods declaring relations to the target, following incoming edges.
    #[arg(short = 'R', long)]
    reverse: bool,
    #[command(flatten)]
    filter: filter::FilterArgs,
}

#[derive(Debug, Error)]
enum ViewError {
    #[error("no graph input: specify -i FILE or pipe graph JSON on stdin")]
    MissingInput,
    #[error("dependency graph has no node {identifier:?}")]
    UnknownMod { identifier: DependencyIdentifier },
}

pub(super) fn run(args: ViewArgs) -> Result<()> {
    let graph = if let Some(path) = args.input.filter(|path| path.as_os_str() != "-") {
        let input = File::open(&path)
            .with_context(|| format!("failed to open dependency graph {path:?}"))?;
        storage::read_json(input, &BUILTIN_LOADERS)
            .with_context(|| format!("failed to restore dependency graph {path:?}"))?
    } else {
        let input = io::stdin();
        if input.is_terminal() {
            return Err(ViewError::MissingInput.into());
        }
        storage::read_json(input.lock(), &BUILTIN_LOADERS)
            .context("failed to read dependency graph from stdin")?
    };
    let identifier = args.target.0;
    let filter = args.filter.into_filter();
    let depths = if args.reverse {
        graph.node(&identifier).map(|_| HashMap::new())
    } else {
        graph.dependency_depths_with(&identifier, |target, dependency| {
            filter.includes(target, dependency)
        })
    }
    .ok_or_else(|| ViewError::UnknownMod {
        identifier: identifier.clone(),
    })?;
    let mut output = io::stdout().lock();
    output::write_view(
        &mut output,
        &graph,
        &identifier,
        depths,
        &filter,
        args.reverse,
    )
    .context("failed to write dependency graph view")?;
    output
        .flush()
        .context("failed to flush dependency graph view")?;
    Ok(())
}
