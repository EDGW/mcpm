//! Resolves directory and loader inputs for shared dependency graph commands.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mcmod::ModDirectory;

use crate::commands::mod_support::{
    dep_graph::{self, BuildOptions},
    loaders::LoaderArgs,
};

pub(super) type DepGraphArgs = dep_graph::DepGraphArgs<BuildArgs>;

#[derive(Args, Debug)]
pub(super) struct BuildArgs {
    /// Directory containing .jar and .jar.disabled files (not scanned recursively).
    #[arg(value_name = "DIR")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    options: BuildOptions,
}

pub(super) fn run(args: DepGraphArgs) -> Result<()> {
    args.run(|args| {
        let directory = ModDirectory::new(args.directory)
            .context("failed to open dependency graph input directory")?;
        args.options.run(&directory, &args.loaders.select(), [])
    })
}
