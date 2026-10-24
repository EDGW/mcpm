//! Read persisted server metadata locally or through a structured SSH data source.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use super::{
    metadata::{self, ServerInfo},
    output,
    target::{self, Target},
    workspace,
};
use crate::ssh::SshArgs;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Remote: supported through mcpm remote (command ID: server.info) and --ssh.\n\nRemote workspaces also show their saved remote path, masked SSH command and remote mcpm executable. SSH commands remain masked with --full-command. With --ssh, PATH is resolved on the remote machine; output is formatted locally. Startup commands retain the source server’s Windows or Unix argument quoting."
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
    let Target { path, ssh, remote } = target::resolve(args.path, ssh)?;
    let info = if ssh.is_requested() {
        metadata::load_ssh(ssh, path)?
    } else {
        tracing::info!(path = %path.display(), "Reading server metadata");
        let server = workspace::inspect_local(path)?;
        ServerInfo::from_server(&server)
    };
    let mut stdout = io::stdout().lock();
    output::write_snapshot(&mut stdout, &info, args.full_command, remote.as_ref())
        .context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
