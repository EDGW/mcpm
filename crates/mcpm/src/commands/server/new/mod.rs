//! Creates a vanilla server (optionally installing a mod loader immediately after) through
//! `server.new`; EULA interaction and presentation stay on the frontend.

use std::{io, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use clap::Args;
use mcpm_backend::operations::server::new::{New, NewEvent, NewInput, Prepared, RconOptions};
use mcpm_lib::local::codec::Codec;
use mcpm_rich::rich;

use super::loader_spec::LoaderSpec;
use crate::{
    cli::source::SourceArgs,
    commands::{CommandOutcome, execution::Runtime},
    display::{console::ConsoleWriter, rich::block, style::OutputStyle},
    progress::Progress,
    ssh::SshArgs,
};

mod eula;
mod output;

/// Builds creation behavior and restrictions in reading order.
fn after_help() -> clap::builder::StyledStr {
    use super::after_help::{Help, bold};

    let mut help = Help::new();
    help.heading("Creation");
    help.blank();
    help.line("  The parent directory must exist; the target must be missing or empty.");
    help.line("  The selected Minecraft and optional loader versions are validated before EULA confirmation.");
    help.line("  Failed vanilla creation preserves completed files; inspect them before retrying.");
    help.blank();
    help.heading("Mod Loader");
    help.blank();
    help.line(&format!(
        "  Use {} or {} {} to install a loader immediately after vanilla setup.",
        bold("--loader <forge|fabric|neoforge>[:<version>]"),
        bold("--loader <name>"),
        bold("--loader-version <version>")
    ));
    help.line("  If loader installation fails after vanilla creation succeeds, the clean Vanilla server is kept and the command exits non-zero.");
    help.blank();
    help.heading("RCON");
    help.blank();
    help.line("  Releases at or after 1.9-pre4 get RCON settings in server.properties.");
    help.line(&format!(
        "  Earlier releases reject explicit {} and {}.",
        bold("--rcon-port"),
        bold("--rcon-password")
    ));
    help.blank();
    help.heading("EULA");
    help.blank();
    help.line(&format!(
        "  {} requires an interactive terminal and defaults to no.",
        bold("--eula")
    ));
    help.line("  Without consent, eula.txt contains eula=false; review it before launching.");
    help.blank();
    help.heading("Modes");
    help.blank();
    help.line(&format!(
        "  Console; Raw with {} or redirected stdout.",
        bold("--raw")
    ));
    help.finish()
}

#[derive(Args, Debug)]
#[command(after_help = after_help())]
pub(super) struct NewArgs {
    /// New or empty server directory; defaults to the current directory.
    #[arg(value_name = "DIR_PATH", default_value = ".")]
    path: PathBuf,
    /// Exact Minecraft version, latest release, or latest-snapshot.
    #[arg(long, value_name = "VERSION", default_value = "latest")]
    version: String,
    /// Optional mod loader to install after vanilla creation: <forge|fabric|neoforge>[:<version>].
    #[arg(long, value_name = "LOADER[:VERSION]")]
    loader: Option<LoaderSpec>,
    /// Exact loader version; requires --loader and conflicts with inline :<version>.
    #[arg(long, value_name = "VERSION", requires = "loader")]
    loader_version: Option<String>,
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
    /// RCON password; omitted passwords are fresh random GUIDs. Shown in the creation result.
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

pub(super) fn run(args: NewArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = Runtime::new()?;
    let result = runtime.block_on(run_async(args, ssh));
    // Allow local blocking loader cancellation cleanup to terminate any installer process tree
    // and remove `.mcpm/loader-install/<id>/` before exiting.
    runtime.shutdown_timeout(Duration::from_secs(5));
    result
}

async fn run_async(args: NewArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let loader = args
        .loader
        .map(|spec| spec.into_request(args.loader_version))
        .transpose()?;
    let config = args.source.download_config()?;
    let (executor, path, remote) = ssh.create_executor(Some(args.path)).await?;
    let path = path.context("server creation requires a path")?;
    let prepared = prepare(
        &executor,
        NewInput::Prepare {
            path: path.clone(),
            version: args.version,
            download_config: config.encode()?,
            rcon_requested: args.rcon_port.is_some() || args.rcon_password.is_some(),
            loader: loader.clone(),
        },
    )
    .await?;
    let accepted = if args.eula {
        if !eula::accept(&config).await? {
            return Ok(CommandOutcome::Completed);
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
    let progress = Progress::new(
        if loader.is_some() {
            "Creating server and installing loader..."
        } else {
            "Creating vanilla server..."
        },
        "Creating server",
    );
    let mut in_bytes = false;
    let mut previous_bytes = 0;
    let mut result = None;
    let mut publishing = false;
    let mut publishing_interrupt_notified = false;
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            signal = &mut ctrl_c => {
                signal.context("failed to listen for Ctrl+C")?;
                if publishing {
                    if !publishing_interrupt_notified {
                        publishing_interrupt_notified = true;
                        progress.suspend(|| {
                            let mut stderr = io::stderr().lock();
                            let mut writer = ConsoleWriter::new(&mut stderr);
                            let _ = block(
                                &mut writer,
                                &rich!(
                                    "[Notice:](warning) Publication is already in progress; waiting for atomic commit to finish..."
                                ),
                                &OutputStyle::stderr(),
                            );
                        });
                    }
                    ctrl_c.set(tokio::signal::ctrl_c());
                    continue;
                }
                drop(progress);
                drop(stream);
                tokio::task::yield_now().await;
                drop(executor);
                bail!("server creation cancelled");
            }
            next = stream.next() => {
                let Some(event) = next else {
                    break;
                };
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
                    NewEvent::Notice(notice) => {
                        progress.suspend(|| {
                            let mut stderr = io::stderr().lock();
                            let mut writer = ConsoleWriter::new(&mut stderr);
                            let _ = block(
                                &mut writer,
                                &rich!("[Notice:](warning) {notice}"),
                                &OutputStyle::stderr(),
                            );
                        });
                    }
                    NewEvent::Publishing { message } => {
                        publishing = true;
                        progress.phase(&message);
                        in_bytes = false;
                    }
                    NewEvent::Installed(installed) => result = Some(*installed),
                    NewEvent::Prepared(_) => {
                        bail!("server installation returned an unexpected preparation result")
                    }
                }
            }
        }
    }
    drop(progress);

    let mut installed = result.context("server creation stream ended without its result")?;
    let loader_error = installed.loader_error.take();
    output::write(
        &installed,
        args.full_command,
        accepted,
        remote.as_ref(),
        loader_error.as_ref(),
        loader.as_ref(),
    )?;
    Ok(if loader_error.is_some() {
        CommandOutcome::Exited(1)
    } else {
        CommandOutcome::Completed
    })
}

/// Resolve and freeze exact Minecraft (and optional loader) artifacts before EULA consent.
async fn prepare(executor: &mcpm_exec::service::Executor, input: NewInput) -> Result<Prepared> {
    let progress = Progress::new(
        "Checking Minecraft version...",
        "Resolving server version",
    );
    let mut stream = executor.call::<New>(input).await?;
    let mut prepared = None;
    while let Some(event) = stream.next().await {
        match event? {
            NewEvent::Progress { message, .. } | NewEvent::Publishing { message } => {
                progress.phase(&message);
            }
            NewEvent::Notice(notice) => {
                progress.suspend(|| {
                    let mut stderr = io::stderr().lock();
                    let mut writer = ConsoleWriter::new(&mut stderr);
                    let _ = block(
                        &mut writer,
                        &rich!("[Notice:](warning) {notice}"),
                        &OutputStyle::stderr(),
                    );
                });
            }
            NewEvent::Prepared(value) => prepared = Some(*value),
            NewEvent::Installed(_) => bail!("server preparation unexpectedly installed a server"),
        }
    }
    prepared.context("server preparation stream ended without a resolved version")
}
