mod output;

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mc::server::MinecraftServer;

#[derive(Args)]
pub(super) struct InfoArgs {
    /// Server directory; defaults to the current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
}

pub(super) fn run(args: InfoArgs) -> Result<()> {
    let server = MinecraftServer::load(args.path)?;
    let mut stdout = io::stdout().lock();
    output::write_info(&mut stdout, &server).context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
