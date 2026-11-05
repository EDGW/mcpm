//! Read persisted server metadata locally or through a structured SSH data source.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{commands::execution::Runtime, ssh::SshArgs};
use mcpm_backend::operations::server::info::{Info, InfoInput};

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Explicit --ssh selects the remote host; otherwise workspace registration selects Local or Remote execution through mcpmd. Remote connection details remain masked with --full-command. Info reports launch.toml and conf.toml presence without preparing or starting a launch."
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
    let runtime = Runtime::new()?;
    let (info, remote) = runtime.block_on(async {
        let (executor, path, remote) = ssh.create_executor(Some(args.path)).await?;
        let info = executor
            .call::<Info>(InfoInput {
                path: path.context("server info requires a path")?,
            })
            .await?;
        Ok::<_, anyhow::Error>((info, remote))
    })?;
    let mut stdout = io::stdout().lock();
    crate::display::server::write_snapshot(&mut stdout, &info, args.full_command, remote.as_ref())
        .context("failed to write server information")?;
    stdout.flush().context("failed to flush server information")
}
