//! Stash CLI routing and execution-location resolution; each leaf owns its arguments and presentation.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context as _, Result};
use clap::{Args, Subcommand};
use mcpm_exec::service::Executor;

use crate::{commands::execution::Runtime, ssh::SshArgs};

mod columns;
mod create;
mod delete;
mod info;
mod input;
mod list;
mod options;
mod query;
mod rename;
mod status;
mod switch;
mod verify;

pub(super) fn run(args: StashArgs, ssh: SshArgs) -> Result<()> {
    let runtime = Runtime::new()?;
    runtime.block_on(async {
        let (executor, path, _) = ssh.create_executor(Some(args.server)).await?;
        let context = Context {
            path: path.context("stash requires a server path")?,
            headers: args.headers,
            executor,
        };
        let mut out = io::stdout().lock();
        match args.command {
            Command::Create(args) => create::run(args, &context, &mut out).await?,
            Command::Switch(args) => switch::run(args, &context, &mut out).await?,
            Command::Status(args) => status::run(args, &context, &mut out).await?,
            Command::List(args) => list::run(args, &context, &mut out).await?,
            Command::Info(args) => info::run(args, &context, &mut out).await?,
            Command::Rename(args) => rename::run(args, &context, &mut out).await?,
            Command::Delete(args) => delete::run(args, &context, &mut out).await?,
            Command::Verify(args) => verify::run(args, &context, &mut out).await?,
        }
        out.flush().context("failed to flush stash output")
    })
}

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout). Explicit --ssh selects the host; otherwise workspace registration selects Local or Remote execution. Snapshots include every regular file in mods/, including disabled mods and non-JAR files. Empty directories are omitted. Backup manifests reference contents in the server-local stash cache; keep the stash directory together with its backups. Keep mods stable during snapshot creation and switching. No automatic interruption recovery."
)]
pub(super) struct StashArgs {
    /// Server directory on the owning host, or a registered Remote workspace.
    #[arg(long, global = true, default_value = ".", value_name = "PATH")]
    server: PathBuf,
    /// Include Raw TSV column headers; ignored in Console.
    #[arg(long, global = true)]
    headers: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Save changed or untracked mods and advance HEAD (Console, Raw).
    Create(create::CreateArgs),
    /// Verify a snapshot, then replace mods and advance HEAD (Console, Raw).
    Switch(switch::SwitchArgs),
    /// Compare mods against HEAD; clean/modified/untracked are successful results (Console, Raw).
    Status(status::StatusArgs),
    /// List saved versions, newest first; HEAD marks the baseline only (Console, Raw).
    List(list::ListArgs),
    /// Read selected indices and manifests; --details lists files in Console (Console, Raw).
    Info(info::InfoArgs),
    /// Change one name without changing UUID, time, backup or HEAD (Console, Raw).
    Rename(rename::RenameArgs),
    /// Delete selected snapshots, preserving mods and the content cache (Console, Raw).
    Delete(delete::DeleteArgs),
    /// Fully verify saved snapshots without modifying files or HEAD (Console, Raw).
    Verify(verify::VerifyArgs),
}

/// Family-wide CLI settings and resolved execution target; no storage lease is held here.
struct Context {
    path: PathBuf,
    headers: bool,
    executor: Executor,
}
