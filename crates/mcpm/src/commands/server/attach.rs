//! `server attach`: observe a server instance without owning its lifecycle.

use std::{io::Write, path::PathBuf};

use anyhow::Result;
use clap::Args;
use mcpm_backend::server::instance::{Attach, AttachEvent, AttachInput};

use crate::{
    commands::{CommandOutcome, execution},
    ssh::SshArgs,
};

#[derive(Debug, Args)]
pub(super) struct AttachArgs {
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    #[arg(long, default_value_t = 100)]
    tail: usize,
}

pub(super) fn run(args: AttachArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = execution::Runtime::new()?;
    let session = execution::session_for(&ssh)?;
    let input = AttachInput {
        path: args.server,
        tail: args.tail,
    };
    let mut handle = runtime
        .block_on(session.start::<Attach>(input))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    let stdout = std::sync::Arc::new(std::sync::Mutex::new(std::io::stdout()));
    loop {
        match runtime.block_on(handle.next()) {
            Some(AttachEvent { text, .. }) => {
                let mut output = stdout.lock().expect("stdout mutex poisoned");
                writeln!(output, "{text}")?;
                output.flush()?;
            }
            None => break,
        }
    }
    runtime
        .block_on(handle.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    Ok(CommandOutcome::Completed)
}
