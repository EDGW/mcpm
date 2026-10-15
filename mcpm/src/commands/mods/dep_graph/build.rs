use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mcmod::{
    ModDirectory,
    dependency::graph::{build, candidate::ExternalCandidate, storage},
};

use super::super::loaders::LoaderArgs;
use super::selector::ModSelector;

#[derive(Args)]
pub(super) struct BuildArgs {
    /// Directory containing .jar and .jar.disabled files (not scanned recursively).
    #[arg(value_name = "DIR")]
    directory: PathBuf,
    /// Write JSON to this file instead of stdout; overwrite an existing file.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Add an external candidate as loader:modid; repeat to add more.
    #[arg(long, value_name = "LOADER:MODID")]
    external: Vec<ModSelector>,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: BuildArgs) -> Result<()> {
    let directory = ModDirectory::new(args.directory)
        .context("failed to open dependency graph input directory")?;
    let mut graph = build::build(
        &directory,
        &args.loaders.select(),
        crate::diagnostics::error,
    )?;
    let mut added = HashSet::new();
    for ModSelector(identifier) in args.external {
        if added.insert(identifier.clone()) {
            graph.add_external(ExternalCandidate { identifier });
        }
    }

    if let Some(path) = args.output {
        let file = File::create(&path)
            .with_context(|| format!("failed to create dependency graph output {path:?}"))?;
        let mut output = BufWriter::new(file);
        storage::write_json(&graph, &mut output)
            .with_context(|| format!("failed to write dependency graph to {path:?}"))?;
        output
            .flush()
            .with_context(|| format!("failed to flush dependency graph output {path:?}"))?;
    } else {
        let mut output = io::stdout().lock();
        storage::write_json(&graph, &mut output)
            .context("failed to write dependency graph to stdout")?;
        output
            .flush()
            .context("failed to flush dependency graph stdout")?;
    }
    Ok(())
}
