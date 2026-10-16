//! Lists user-supplied mod paths using the shared CLI listing pipeline.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::commands::mod_support::{
    list::{self, ListDisplayArgs},
    loaders::LoaderArgs,
    stdin,
};

mod input;

#[derive(Args)]
pub(super) struct ListArgs {
    /// Mod files or directories; append paths from stdin, one per line. Defaults to . when empty.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    display: ListDisplayArgs,
    #[command(flatten)]
    loaders: LoaderArgs,
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let mut paths = stdin::append(args.paths)?;
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let inputs = paths.into_iter().map(|path| {
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
