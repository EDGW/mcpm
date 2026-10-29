//! Read persisted server metadata locally or through a structured SSH data source.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use super::{
    metadata::{self, ServerInfo},
    target::{self, Target},
    workspace,
};
use crate::ssh::SshArgs;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Remote: supported through {tools} remote (command ID: server.info) and --ssh.\n\nRemote workspaces also show their saved remote path, masked SSH command and remote mcpm executable. SSH commands remain masked with --full-command. With --ssh, PATH is resolved on the remote machine; output is formatted locally. Legacy command snapshots retain source Windows/Unix quoting. Local Console output reports launch.toml and conf.toml presence; info does not prepare or run a launch.", tools = crate::program::roots().tools)
)]
pub(super) struct InfoArgs {
    /// Server directory on the data-source machine; defaults to its current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Show the entire legacy command snapshot when present; launch uses the TOML files.
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
        let mut info = ServerInfo::from_server(&server);
        info.configuration =
            Some(mcpm_backend::local::mc::server::config::configuration_files(server.path())?);
        info
    };
    let mut stdout = io::stdout().lock();
    let remote = remote.as_ref().map(metadata::remote_view);
    crate::display::server::write_snapshot(&mut stdout, &info, args.full_command, remote.as_ref())
        .context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
