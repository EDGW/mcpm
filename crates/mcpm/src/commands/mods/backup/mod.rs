//! Backup command dispatch; Host workflows and local presentation have independent owners.

use crate::{commands::execution::Runtime, ssh::SshArgs};
use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_exec::service::Executor;
use tokio::sync::OnceCell;

mod apply;
mod checksum;
mod create;
mod delete;
mod input;
mod verify;
mod verify_with;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Backups recursively store regular files, including disabled mods and other extensions. Empty directories are omitted. Backup, source, target and cache paths belong to the execution host selected by --ssh."
)]
pub(crate) struct BackupArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a verified, read-only file backup (Console, Raw).
    Create(create::CreateArgs),
    /// Verify and apply a file backup to a directory (Console, Raw).
    Apply(apply::ApplyArgs),
    /// Verify one or more backups without applying them (Console, Raw).
    Verify(verify::VerifyArgs),
    /// Compute backup-compatible checksums for directories (Console, Raw).
    Checksum(checksum::ChecksumArgs),
    /// Compare a directory with backup manifests or checksums (Console, Raw).
    VerifyWith(verify_with::VerifyWithArgs),
    /// Delete one or more backups while preserving cached content (Console, Raw).
    Delete(delete::DeleteArgs),
}

pub(crate) fn run(args: BackupArgs, ssh: SshArgs) -> Result<()> {
    let context = Context {
        ssh,
        executor: OnceCell::new(),
    };
    Runtime::new()?.block_on(async {
        match args.command {
            Command::Create(args) => create::run(args, &context).await,
            Command::Apply(args) => apply::run(args, &context).await,
            Command::Verify(args) => verify::run(args, &context).await,
            Command::Checksum(args) => checksum::run(args, &context).await,
            Command::VerifyWith(args) => verify_with::run(args, &context).await,
            Command::Delete(args) => delete::run(args, &context).await,
        }
    })
}

/// Open one executor only after the leaf has collected and validated local input.
struct Context {
    ssh: SshArgs,
    executor: OnceCell<Executor>,
}
impl Context {
    async fn executor(&self) -> Result<&Executor> {
        self.executor
            .get_or_try_init(|| async {
                let (executor, _, _) = self.ssh.create_executor(None).await?;
                Ok(executor)
            })
            .await
    }
}

#[derive(Debug, Args)]
struct DisplayArgs {
    /// Include per-file fields in Console output; ignored by Raw.
    #[arg(long)]
    details: bool,
    /// Include the header in Raw TSV output; ignored by Console.
    #[arg(long)]
    headers: bool,
}
