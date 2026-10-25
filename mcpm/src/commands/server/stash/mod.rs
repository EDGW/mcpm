//! Local stash family routing; each leaf owns its arguments, validation and operation lease.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context as _, Result, ensure};
use clap::{Args, Subcommand};
use mcpm_backend::local::mc::server::MinecraftServer;

use super::{target, workspace};
use crate::ssh::SshArgs;

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
    ensure!(
        !ssh.is_requested(),
        "stash does not support SSH; run it on the server host"
    );
    target::require_supported_local(&args.server, "stash")?;
    let context = Context {
        path: args.server,
        headers: args.headers,
    };
    let mut out = io::stdout().lock();
    match args.command {
        Command::Create(args) => create::run(args, &context, &mut out)?,
        Command::Switch(args) => switch::run(args, &context, &mut out)?,
        Command::Status(args) => status::run(args, &context, &mut out)?,
        Command::List(args) => list::run(args, &context, &mut out)?,
        Command::Info(args) => info::run(args, &context, &mut out)?,
        Command::Rename(args) => rename::run(args, &context, &mut out)?,
        Command::Delete(args) => delete::run(args, &context, &mut out)?,
        Command::Verify(args) => verify::run(args, &context, &mut out)?,
    }
    out.flush().context("failed to flush stash output")
}

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (TSV; --raw or redirected stdout). Local only; SSH and Remote workspaces are rejected. Snapshots include every regular file in mods/, including disabled mods and non-JAR files. Empty directories are omitted. Keep mods stable during snapshot creation and switching. No automatic interruption recovery."
)]
pub(super) struct StashArgs {
    /// Registered local server directory.
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

/// Family-wide CLI settings; opening a server never creates mods or selects a lease mode.
struct Context {
    path: PathBuf,
    headers: bool,
}

impl Context {
    fn server(&self) -> Result<MinecraftServer> {
        workspace::inspect_local(self.path.clone())
    }
}
