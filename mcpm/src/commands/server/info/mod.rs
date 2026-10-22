//! Read persisted server metadata locally or through a structured SSH data source.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mc::server::MinecraftServer;

use super::{metadata::ServerInfo, output};
use crate::ssh::SshArgs;

mod remote;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Remote: supported through mcpm remote (command ID: server.info) and --ssh.\n\nWith --ssh, PATH is resolved on the remote machine; output is formatted locally. Startup commands retain the source server’s Windows or Unix argument quoting."
)]
pub(super) struct InfoArgs {
    /// Server directory on the data-source machine; defaults to its current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Show the entire flattened command instead of truncating it to one terminal line.
    #[arg(long)]
    full_command: bool,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: InfoArgs, ssh: SshArgs) -> Result<()> {
    let info = if ssh.is_requested() {
        remote::load(ssh, args.path)?
    } else {
        tracing::info!(path = %args.path.display(), "Reading server metadata");
        let server = MinecraftServer::load(args.path)?;
        ServerInfo::from_server(&server)
    };
    let mut stdout = io::stdout().lock();
    output::write_snapshot(&mut stdout, &info, args.full_command)
        .context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
