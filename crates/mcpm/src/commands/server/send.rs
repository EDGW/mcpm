//! `server send`: forward one line of input to the running instance.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use mcpm_backend::server::instance::{Send, SendInput};

use crate::{
    commands::{CommandOutcome, execution},
    ssh::SshArgs,
};

#[derive(Debug, Args)]
pub(super) struct SendArgs {
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    #[arg(value_name = "LINE")]
    line: String,
}

pub(super) fn run(args: SendArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = execution::Runtime::new()?;
    let session = execution::session_for(&ssh)?;
    let input = SendInput {
        path: args.server,
        line: args.line,
    };
    let handle = runtime
        .block_on(session.start::<Send>(input))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    runtime
        .block_on(handle.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    Ok(CommandOutcome::Completed)
}
