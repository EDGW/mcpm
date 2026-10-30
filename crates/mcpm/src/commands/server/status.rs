//! `server status`: inspect the host-owned server instance state.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use mcpm_backend::server::instance::{Status, StatusInput};

use crate::{
    commands::{CommandOutcome, execution},
    ssh::SshArgs,
};

#[derive(Debug, Args)]
pub(super) struct StatusArgs {
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
}

pub(super) fn run(args: StatusArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = execution::Runtime::new()?;
    let session = execution::session_for(&ssh)?;
    let input = StatusInput { path: args.server };
    let handle = runtime
        .block_on(session.start::<Status>(input))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    let output = runtime
        .block_on(handle.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    println!("{:?}", output.status);
    Ok(CommandOutcome::Completed)
}
