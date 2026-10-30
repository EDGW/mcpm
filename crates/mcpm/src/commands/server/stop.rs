//! `server stop`: request graceful or forced instance termination.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use mcpm_backend::server::instance::{Stop, StopInput};

use crate::{
    commands::{CommandOutcome, execution},
    ssh::SshArgs,
};

#[derive(Debug, Args)]
pub(super) struct StopArgs {
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    #[arg(long)]
    force: bool,
}

pub(super) fn run(args: StopArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = execution::Runtime::new()?;
    let session = execution::session_for(&ssh)?;
    let input = StopInput {
        path: args.server,
        force: args.force,
    };
    let handle = runtime
        .block_on(session.start::<Stop>(input))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    let output = runtime
        .block_on(handle.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    eprintln!("{:?}", output.status);
    Ok(CommandOutcome::Completed)
}
