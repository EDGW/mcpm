//! Installs a mod loader onto an existing registered v4 Vanilla server via `server.loader.add`.
//!
//! Keeps all version resolution, installer execution, verification, and publication on the
//! selected Host while rendering streaming progress, notices, and Console/Raw server snapshots
//! on the local terminal.

use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Args;
use mcpm_backend::operations::server::{
    info::ServerInfo,
    loader::{AddLoader, AddLoaderEvent, AddLoaderInput, AddLoaderStage},
};
use mcpm_lib::local::codec::Codec;
use mcpm_rich::rich;

use super::loader_spec::LoaderSpec;
use crate::{
    cli::source::SourceArgs,
    commands::execution::Runtime,
    display::{console::ConsoleWriter, rich::block, style::OutputStyle},
    progress::Progress,
    server::RemoteInfo,
    ssh::SshArgs,
    terminal,
};

/// Builds usage and behavior notes in reading order without exposing internal protocols.
fn after_help() -> clap::builder::StyledStr {
    use super::after_help::{Help, bold};

    let mut help = Help::new();
    help.heading("Eligibility");
    help.blank();
    help.line("  Converts an existing v4 Vanilla server registered in .mcpm/server.json in place.");
    help.line("  Modded, unrecognized, legacy (v1-v3), or currently running servers are rejected.");
    help.line("  The server's launch.toml must use a -jar entry matching its registered version.");
    help.blank();
    help.heading("Files");
    help.blank();
    help.line("  Updates .mcpm/server.json, launch.toml, and required loader runtime files.");
    help.line("  Preserves conf.toml byte-for-byte and never overwrites the original vanilla JAR.");
    help.line(&format!(
        "  Use {} to replace conflicting non-vanilla runtime files.",
        bold("--force")
    ));
    help.blank();
    help.heading("Modes");
    help.blank();
    help.line(&format!(
        "  Console; Raw with {} or redirected stdout (6-column server snapshot).",
        bold("--raw")
    ));
    help.finish()
}

#[derive(Args, Debug)]
#[command(after_help = after_help())]
pub(super) struct AddLoaderArgs {
    /// Target mod loader: <forge|fabric|neoforge>[:<version>].
    #[arg(value_name = "LOADER[:VERSION]")]
    loader: LoaderSpec,
    /// Registered Vanilla server directory; defaults to the current directory.
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    /// Exact loader version to install; conflicts with inline <loader>:<version>.
    #[arg(long, value_name = "VERSION")]
    loader_version: Option<String>,
    #[command(flatten)]
    source: SourceArgs,
    /// Replace conflicting non-vanilla runtime files (never overwrites the original vanilla JAR).
    #[arg(long)]
    force: bool,
    /// Show the complete generated startup command.
    #[arg(long)]
    full_command: bool,
}

pub(super) fn run(args: AddLoaderArgs, ssh: SshArgs) -> Result<()> {
    let runtime = Runtime::new()?;
    let result = runtime.block_on(run_async(args, ssh));
    // Give local blocking cancellation cleanup time to terminate the installer process tree
    // and remove `.mcpm/loader-install/<id>/` before the process exits.
    runtime.shutdown_timeout(Duration::from_secs(5));
    result
}

async fn run_async(args: AddLoaderArgs, ssh: SshArgs) -> Result<()> {
    let request = args.loader.into_request(args.loader_version)?;
    let config = args.source.download_config()?;
    let (executor, path, remote) = ssh.create_executor(Some(args.server)).await?;
    let path = path.context("add-loader requires a server path")?;
    let family = request.family;

    let mut stream = executor
        .call::<AddLoader>(AddLoaderInput {
            path,
            family,
            loader_version: request.version,
            download_config: config.encode()?,
            force: args.force,
        })
        .await?;

    let progress = Progress::new(&format!("Preparing {family} installation..."), "Adding loader");
    let mut in_bytes = false;
    let mut previous_bytes = 0;
    let mut completed = None;
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
                // Let the local cancel watcher run or the remote session flush the abort frame.
                tokio::task::yield_now().await;
                drop(executor);
                bail!("loader installation cancelled");
            }
            next = stream.next() => {
                let Some(event) = next else {
                    break;
                };
                match event? {
                    AddLoaderEvent::Stage { stage, message } => {
                        if stage == AddLoaderStage::Publish {
                            publishing = true;
                        }
                        progress.phase(&message);
                        in_bytes = false;
                    }
                    AddLoaderEvent::DownloadProgress {
                        message,
                        received,
                        total,
                    } => {
                        if !in_bytes || received < previous_bytes || received == 0 {
                            progress.start_bytes(&message);
                        }
                        progress.set_message(&message);
                        progress.bytes(received, total);
                        previous_bytes = received;
                        in_bytes = true;
                    }
                    AddLoaderEvent::InstallerAttempt { source, attempt } => {
                        progress.phase(&format!(
                            "Running {family} installer (attempt {attempt} · {source})..."
                        ));
                        in_bytes = false;
                    }
                    AddLoaderEvent::Notice(notice) => {
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
                    AddLoaderEvent::Completed(server) => {
                        completed = Some(*server);
                    }
                }
            }
        }
    }
    drop(progress);

    let server = completed.context("add-loader stream ended without its server snapshot")?;
    write_output(&server, args.full_command, remote.as_ref())
}

fn write_output(
    server: &ServerInfo,
    full_command: bool,
    remote: Option<&RemoteInfo>,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    crate::display::server::write_snapshot(&mut stdout, server, full_command, remote)
        .context("failed to write converted server information")?;
    if terminal::stdout_is_tty() {
        let mut output = ConsoleWriter::new(&mut stdout);
        writeln!(output)?;
        block(
            &mut output,
            &rich!("[Loader installed successfully.](success)"),
            &OutputStyle::new(),
        )?;
    }
    stdout
        .flush()
        .context("failed to flush converted server information")
}
