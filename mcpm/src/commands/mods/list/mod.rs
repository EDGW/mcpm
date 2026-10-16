//! Lists user-supplied mod paths using the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs},
    loaders::LoaderArgs,
};

mod input;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Mod files or directories, processed in argument order (directories are not recursive).
    #[arg(value_name = "PATH", default_value = ".", num_args = 1..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let inputs = args.paths.into_iter().map(|path| {
        let context = format!("failed to read list input {path:?}");
        input::read(path).context(context)
    });
    list::run(
        inputs,
        args.loaders.select(),
        args.loaders.is_filtered(),
        args.display,
    )
}
