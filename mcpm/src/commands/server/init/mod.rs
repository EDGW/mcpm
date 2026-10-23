//! Server initialization frontend: direct complete arguments, or read-only recognition followed by local selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use super::{metadata::ServerInfo, output};
use crate::{commands::progress::Progress, remote::client::Session, ssh::SshArgs};
use model::{Initialize, Phase, Recognize};

mod model;
mod operation;
mod remote;
mod selector;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: TUI (multiple-candidate selection when stdout and stderr are terminals), Console (--no-ui), Raw (--raw or redirected stdout). Console and Raw require an unambiguous candidate or complete manual arguments.\n\nRemote supported through --ssh. Paths, script recognition, platform defaults and metadata writes belong to the SSH host; selection and output stay local.\nComplete --minecraft-version and --loader (plus --loader-version unless vanilla) initialize directly without reading scripts. Otherwise, recognition returns candidates before initialization.\nRecognition never writes configuration. Initialization accepts the selected complete snapshot without rereading its script. --force allows atomic replacement; a failed recognition or cancelled selection leaves existing metadata intact.\nCancellation is checked before writing. A disconnect or cancellation during/after the atomic write may leave initialization completed; inspect server info to check the result."
)]
pub(super) struct InitArgs {
    /// Existing server directory on the data-source host; defaults to its current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
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
    /// Atomically replace existing server.json after validation and any candidate selection.
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
    let recognition = Recognize {
        path: args.path,
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
        Some(Session::start(&mut ssh.command()?).context("failed to open SSH mcpm session")?)
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
    let mut stdout = io::stdout().lock();
    output::write_snapshot(&mut stdout, &info, args.full_command)
        .context("failed to write initialized server information")?;
    stdout
        .flush()
        .context("failed to flush initialized server information")
}

/// CLI choices come from the same loader registry used by recognition and initialization.
fn loader_parser() -> clap::builder::PossibleValuesParser {
    let mut loaders: Vec<_> = kako_mods_lib::local::modloader::BUILTIN_LOADERS
        .keys()
        .map(|id| id.as_str())
        .collect();
    loaders.push("vanilla");
    loaders.sort_unstable();
    clap::builder::PossibleValuesParser::new(loaders)
}
