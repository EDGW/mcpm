//! Debug commands for acquiring and holding mcpm OS file locks.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use mcpm_backend::operations::debug::lock::{
    LockServer, ServerLockEvent, ServerLockInput, ServerLockMode, ServerLockName,
};
use mcpm_rich::rich;

use crate::{
    commands::execution::Runtime,
    display::{rich::line, style::OutputStyle},
    ssh::SshArgs,
};

#[derive(Args, Debug)]
pub(super) struct LockArgs {
    #[command(subcommand)]
    command: LockCommand,
}

#[derive(Subcommand, Debug)]
enum LockCommand {
    /// Acquire and hold a server lock until interrupted with Ctrl+C.
    Server(ServerLockArgs),
}

#[derive(Args, Debug)]
#[command(
    after_help = "Acquires the requested OS file lock on the selected host (local by default, or remote with --ssh) and holds it until Ctrl+C is pressed. Fails immediately if the lock cannot be acquired or if a shared lock file does not exist."
)]
struct ServerLockArgs {
    /// Server directory on the selected host.
    #[arg(value_name = "SERVER")]
    server: PathBuf,
    /// Which server lock to acquire.
    #[arg(long = "name", value_enum, value_name = "NAME")]
    name: LockNameArg,
    /// Lock mode to acquire.
    #[arg(long = "type", value_enum, value_name = "TYPE")]
    lock_type: LockTypeArg,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum LockNameArg {
    /// Server coordination lock (.mcpm/server.lock).
    Server,
    /// Running-instance liveness lock (.mcpm/run/running.lock).
    Running,
}

impl From<LockNameArg> for ServerLockName {
    fn from(value: LockNameArg) -> Self {
        match value {
            LockNameArg::Server => Self::Server,
            LockNameArg::Running => Self::Running,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum LockTypeArg {
    /// Shared (read) lock; requires the lock file to already exist.
    Shared,
    /// Exclusive (write) lock; creates the lock file if missing.
    Exclusive,
}

impl From<LockTypeArg> for ServerLockMode {
    fn from(value: LockTypeArg) -> Self {
        match value {
            LockTypeArg::Shared => Self::Shared,
            LockTypeArg::Exclusive => Self::Exclusive,
        }
    }
}

pub(super) fn run(args: LockArgs, ssh: SshArgs) -> Result<()> {
    match args.command {
        LockCommand::Server(args) => run_server(args, ssh),
    }
}

fn run_server(args: ServerLockArgs, ssh: SshArgs) -> Result<()> {
    let runtime = Runtime::new()?;
    runtime.block_on(async move {
        let (executor, _, _) = ssh.create_executor(None).await?;
        let mut stream = executor
            .call::<LockServer>(ServerLockInput {
                server: args.server,
                name: args.name.into(),
                mode: args.lock_type.into(),
            })
            .await?;
        let ServerLockEvent::Acquired { path, name, mode } = match stream.next().await {
            Some(Ok(event)) => event,
            Some(Err(error)) => return Err(error),
            None => bail!("lock stream ended before acquiring lock"),
        };
        let style = OutputStyle::new();
        write_acquired(&path, name, mode, &style)?;
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);
        tokio::select! {
            signal = &mut interrupt => {
                signal.context("failed to listen for Ctrl+C")?;
                drop(stream);
                // Let the local producer drop or the remote session writer flush the abort frame
                // before tearing down the executor and runtime.
                tokio::task::yield_now().await;
                drop(executor);
                write_released(&path, &style)?;
                Ok(())
            }
            next = stream.next() => match next {
                Some(Ok(_)) => bail!("unexpected extra event from lock stream"),
                Some(Err(error)) => Err(error).context("lock stream failed while holding lock"),
                None => bail!("lock stream closed unexpectedly before Ctrl+C"),
            }
        }
    })
}

fn write_acquired(
    path: &Path,
    name: ServerLockName,
    mode: ServerLockMode,
    style: &OutputStyle,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let path = path.display();
    writeln!(
        stdout,
        "{}",
        line(
            &rich!("[Acquired](success) [{mode}](secondary) [{name}](primary) lock: `{path}`"),
            style
        )
    )
    .context("failed to write lock acquisition notice")?;
    writeln!(
        stdout,
        "{}",
        line(&rich!("[Press Ctrl+C to release](muted)"), style)
    )
    .context("failed to write lock release hint")?;
    stdout
        .flush()
        .context("failed to flush lock acquisition notice")
}

fn write_released(path: &Path, style: &OutputStyle) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let path = path.display();
    writeln!(
        stdout,
        "{}",
        line(&rich!("[Released](success) lock: `{path}`"), style)
    )
    .context("failed to write lock release notice")?;
    stdout
        .flush()
        .context("failed to flush lock release notice")
}
