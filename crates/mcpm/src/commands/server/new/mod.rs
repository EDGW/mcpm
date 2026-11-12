//! Creates a vanilla server through a Host operation; EULA interaction stays on the frontend.

use anyhow::{Context, Result, bail};
use clap::Args;
use mcpm_backend::operations::server::{
    info::{Info, InfoInput},
    new::{New, NewEvent, NewInput, Prepared, RconOptions},
};
use mcpm_lib::local::codec::Codec;
use std::path::PathBuf;

use crate::{
    cli::source::SourceArgs, commands::execution::Runtime, progress::Progress, ssh::SshArgs,
};

mod eula;
mod output;

#[derive(Args, Debug)]
#[command(after_help = super::after_help::new())]
pub(super) struct NewArgs {
    /// New or empty server directory; defaults to the current directory.
    #[arg(value_name = "DIR_PATH", default_value = ".")]
    path: PathBuf,
    /// Exact Minecraft version, latest release, or latest-snapshot.
    #[arg(long, value_name = "VERSION", default_value = "latest")]
    version: String,
    #[command(flatten)]
    source: SourceArgs,
    /// Fetch the current EULA and ask whether to accept it; only y/yes accepts (default: no).
    #[arg(long, conflicts_with = "raw")]
    eula: bool,
    /// Do not generate server.properties for RCON.
    #[arg(long, conflicts_with_all = ["rcon_port", "rcon_password"])]
    no_rcon: bool,
    /// RCON port for versions released since 1.9-pre4 (default: 25575).
    #[arg(long, value_name = "PORT")]
    rcon_port: Option<u16>,
    /// RCON password; omitted passwords are fresh random GUIDs. Written only, never displayed.
    #[arg(long, value_name = "PASSWORD")]
    rcon_password: Option<String>,
    /// Skip Git discovery and initialization on the actual server Host.
    #[arg(long)]
    no_git: bool,
    /// Ensure the actual server directory itself has a Git repository.
    #[arg(long, conflicts_with = "no_git")]
    git: bool,
    /// Show the complete generated startup command.
    #[arg(long)]
    full_command: bool,
}

pub(super) fn run(args: NewArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(run_async(args, ssh))
}

async fn run_async(args: NewArgs, ssh: SshArgs) -> Result<()> {
    let config = args.source.download_config()?;
    let (executor, path, _) = ssh.create_executor(Some(args.path)).await?;
    let path = path.context("server creation requires a path")?;
    let prepared = prepare(
        &executor,
        NewInput::Prepare {
            path: path.clone(),
            version: args.version,
            download_config: config.encode()?,
            rcon_requested: args.rcon_port.is_some() || args.rcon_password.is_some(),
        },
    )
    .await?;
    let accepted = if args.eula {
        if !eula::accept(&config).await? {
            return Ok(());
        }
        true
    } else {
        false
    };
    let rcon = if args.no_rcon {
        None
    } else {
        Some(RconOptions {
            port: args.rcon_port,
            password: args.rcon_password,
        })
    };
    let mut stream = executor
        .call::<New>(NewInput::Install {
            prepared,
            accept_eula: accepted,
            no_git: args.no_git,
            git: args.git,
            rcon,
        })
        .await?;
    let progress = Progress::new("Creating vanilla server...", "Creating server");
    let mut in_bytes = false;
    let mut previous_bytes = 0;
    let mut result = None;
    while let Some(event) = stream.next().await {
        match event? {
            NewEvent::Progress { message, bytes } => match bytes {
                Some((received, total)) => {
                    if !in_bytes || received < previous_bytes || received == 0 {
                        progress.start_bytes(&message);
                    }
                    progress.set_message(&message);
                    progress.bytes(received, total);
                    previous_bytes = received;
                    in_bytes = true;
                }
                None => {
                    progress.phase(&message);
                    in_bytes = false;
                }
            },
            NewEvent::Installed(installed) => result = Some(installed),
            NewEvent::Prepared(_) => {
                bail!("server installation returned an unexpected preparation result")
            }
        }
    }
    drop(progress);
    let mut installed = result.context("server creation stream ended without its result")?;
    if args.full_command {
        installed.server = executor
            .call::<Info>(InfoInput {
                path,
                full_command: true,
                allow_legacy: false,
            })
            .await?;
    }
    output::write(&installed, args.full_command, accepted)
}

/// Resolve and freeze the exact artifact before any EULA network work or consent.
async fn prepare(executor: &mcpm_exec::service::Executor, input: NewInput) -> Result<Prepared> {
    let progress = Progress::new(
        "Checking Minecraft version...",
        "Resolving Minecraft version",
    );
    let mut stream = executor.call::<New>(input).await?;
    let mut prepared = None;
    while let Some(event) = stream.next().await {
        match event? {
            NewEvent::Progress { message, .. } => progress.phase(&message),
            NewEvent::Prepared(value) => prepared = Some(*value),
            NewEvent::Installed(_) => bail!("server preparation unexpectedly installed a server"),
        }
    }
    prepared.context("server preparation stream ended without a resolved version")
}
