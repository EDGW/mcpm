//! Reads one server snapshot through the `server.info` operation.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::server::{Info, InfoInput};

use super::target;
use crate::commands::execution::{failure as operation_failure, Runtime};
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
    let target = target::resolve(args.path, ssh)?;
    let remote_target = target.session.target().is_remote();
    let runtime = Runtime::new()?;
    let request = InfoInput {
        path: target.path,
        include_configuration: !remote_target,
    };
    let info = runtime.block_on(async {
        let handle = target
            .session
            .start::<Info>(request)
            .await
            .map_err(|error| operation_failure(error, remote_target))?;
        handle
            .finish()
            .await
            .map_err(|error| operation_failure(error, remote_target))
    })?;
    crate::commands::execution::finish(&runtime, &target.session)?;
    let mut stdout = io::stdout().lock();
    crate::display::server::write_snapshot(&mut stdout, &info, args.full_command, target.remote.as_ref())
        .context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
