//! Server initialization frontend: direct complete arguments, or read-only recognition followed by local selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use super::{
    metadata::ServerInfo,
    output,
    target::{self, Target},
};
use crate::{progress::Progress, ssh::SshArgs};
use model::{Initialize, Phase, Recognize};

mod model;
mod operation;
mod reference;
mod remote;
mod selector;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: TUI (candidate selection), Console (--no-ui), Raw (--raw or redirected stdout). Console and Raw require an unambiguous candidate or complete manual arguments.\n\nTwo different SSH operations:\n  mcpm --ssh COMMAND [--ssh-mcpm EXE] server init <REMOTE_PATH>\n    Initialize a Local Server on the SSH host. The positional path is remote.\n  mcpm --ssh COMMAND [--ssh-mcpm EXE] server init <PATH> --remote <REMOTE_PATH>\n    Create a Remote workspace locally. PATH is local; --remote points to an existing remote Local Server.\n\nComplete --minecraft-version and --loader (plus --loader-version unless vanilla) initialize directly without reading scripts. Otherwise, recognition returns candidates before initialization."
)]
pub(super) struct InitArgs {
    /// Existing server directory; with --ssh it is remote, but with --remote it is a local workspace.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Register PATH locally as a Remote workspace pointing to this existing remote Local Server.
    #[arg(long, value_name = "REMOTE_PATH", conflicts_with_all = ["start_script", "minecraft_version", "loader", "loader_version"])]
    remote: Option<String>,
    /// Statically inspect this script; conflicts with manual version/loader options.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["minecraft_version", "loader", "loader_version"])]
    start_script: Option<PathBuf>,
    /// Minecraft version. Omitted fields are inferred from the data-source host's default start script.
    #[arg(long, alias = "mc-version", value_name = "VERSION")]
    minecraft_version: Option<String>,
    /// Loader identity, or vanilla for an explicitly unmodded server.
    #[arg(long, value_parser = loader_parser())]
    loader: Option<String>,
    /// Loader version, independent of Minecraft and FML versions.
    #[arg(long, value_name = "VERSION")]
    loader_version: Option<String>,
    /// Replace existing registration within the same Local/Remote workspace type.
    #[arg(long)]
    force: bool,
    /// Disable the local TUI; multiple candidates require more specific manual filters.
    #[arg(long)]
    no_ui: bool,
    /// Show the entire flattened command in the initialization result.
    #[arg(long)]
    full_command: bool,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: InitArgs, ssh: SshArgs) -> Result<()> {
    if let Some(remote_path) = args.remote {
        let info = reference::create(args.path, remote_path, ssh, args.force)?;
        return write_info(&info, args.full_command);
    }
    let Target { path, ssh, .. } = target::resolve(args.path, ssh)?;
    let recognition = Recognize {
        path,
        start_script: args.start_script,
        minecraft_version: args.minecraft_version,
        loader: args.loader,
        loader_version: args.loader_version,
    };
    operation::validate_options(&recognition)?;
    let manual = recognition.manual_metadata();
    let path = recognition.path.clone();
    let mut session = if ssh.is_requested() {
        tracing::info!("Opening SSH data source for server initialization");
        Some(ssh.connect()?)
    } else {
        None
    };
    let progress = RefCell::new(None);
    let phase = |phase| {
        let description = match phase {
            Phase::Recognizing => "Recognizing server launch candidates...",
            Phase::Validating => "Validating server initialization...",
            Phase::Writing => "Writing server configuration...",
        };
        // Drop the previous progress guard before attaching the next one.
        progress.borrow_mut().take();
        *progress.borrow_mut() = Some(Progress::new(description, "Initializing server"));
        Ok(())
    };
    let metadata = match manual {
        Some(metadata) => metadata,
        None => {
            let mut candidates = match session.as_ref() {
                Some(session) => remote::recognize(session, recognition, phase)?,
                None => operation::recognize(recognition, || Ok(()), phase)?,
            };
            progress.borrow_mut().take();
            let index = if candidates.len() == 1 {
                0
            } else {
                let Some(index) = selector::select(&candidates, args.no_ui)? else {
                    tracing::info!("Server initialization cancelled before writing");
                    if let Some(session) = session.as_mut() {
                        session.close()?;
                    }
                    return Ok(());
                };
                index
            };
            tracing::debug!(
                index,
                candidates = candidates.len(),
                "Selected server launch snapshot"
            );
            candidates.swap_remove(index).metadata
        }
    };
    let initialization = Initialize {
        path,
        metadata,
        force: args.force,
    };
    let info = match session.as_ref() {
        Some(session) => remote::initialize(session, initialization, phase)?,
        None => ServerInfo::from_server(&operation::initialize(initialization, || Ok(()), phase)?),
    };
    progress.borrow_mut().take();
    if let Some(session) = session.as_mut() {
        session.close()?;
    }
    write_info(&info, args.full_command)
}

fn write_info(info: &ServerInfo, full_command: bool) -> Result<()> {
    let mut stdout = io::stdout().lock();
    output::write_snapshot(&mut stdout, info, full_command, None)
        .context("failed to write initialized server information")?;
    stdout
        .flush()
        .context("failed to flush initialized server information")
}

/// CLI choices come from the same loader registry used by recognition and initialization.
fn loader_parser() -> clap::builder::PossibleValuesParser {
    let mut loaders: Vec<_> = mcpm_backend::local::modloader::BUILTIN_LOADERS
        .keys()
        .map(|id| id.as_str())
        .collect();
    loaders.push("vanilla");
    loaders.sort_unstable();
    clap::builder::PossibleValuesParser::new(loaders)
}
