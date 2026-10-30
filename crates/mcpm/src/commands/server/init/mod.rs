//! Server initialization frontend: complete arguments, or recognition followed by selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::server::{
    Candidate, Initialized, Initialize, Phase, Recognize, RecognizeEvent, Recognition, Register,
    RegisterEvent,
};
use mcpm_backend::workspace::{RegisterInput, RemoteRegister, Validate, ValidateInput};
use mcpm_backend::ConnectionSpec;
use super::target;
use crate::commands::execution::{failure as operation_failure, local_session, Runtime};
use crate::progress::Progress;
use crate::ssh::SshArgs;

mod output;
mod selector;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: TUI (candidate selection), Console (--no-ui), Raw (--raw or redirected stdout). Console and Raw require an unambiguous candidate or complete manual arguments.\n\nTwo SSH init flows:\n  {server} --ssh COMMAND [--ssh-mcpmd EXE] init <REMOTE_PATH>\n    Initialize a Local Server on the SSH host.\n  {server} --ssh COMMAND [--ssh-mcpmd EXE] init <PATH> --remote <REMOTE_PATH>\n    Initialize/register host server R, then register local placeholder P. PATH is local; --remote is the host path.\n\nComplete metadata initializes directly on the host; otherwise host recognition returns candidates for caller selection.", server = crate::program::roots().server)
)]
pub(super) struct InitArgs {
    /// Existing server directory; with --ssh it is remote, but with --remote it is local placeholder P.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Initialize host server R, then register PATH as a local Remote workspace placeholder P.
    #[arg(long, value_name = "REMOTE_PATH")]
    remote: Option<String>,
    /// Statically inspect a script inside the server directory, using a relative path; conflicts with manual version/loader options.
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
    /// Replace local registration and launch.toml; preserve existing conf.toml unless --foverride-conf is also set.
    #[arg(long)]
    force: bool,
    #[arg(long, requires = "force")]
    foverride_conf: bool,
    /// Replace caller-side Remote workspace registration; only valid with --remote.
    #[arg(long)]
    replace: bool,
    /// Skip Git repository detection and automatic initialization.
    #[arg(long)]
    no_git: bool,
    /// Create a repository in the target itself even inside a parent repository; reuse an existing target repository.
    #[arg(long, conflicts_with = "no_git")]
    git: bool,
    /// Disable the local TUI; multiple candidates require more specific manual filters.
    #[arg(long)]
    no_ui: bool,
    /// Show an entire legacy command snapshot when one is available.
    #[arg(long)]
    full_command: bool,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(mut args: InitArgs, ssh: SshArgs) -> Result<()> {
    if let Some(remote_path) = args.remote.take() {
        return create_remote(args, remote_path, ssh);
    }
    anyhow::ensure!(!args.replace, "--replace is only valid with --remote");
    let resolved = target::resolve(args.path, ssh)?;
    let remote = resolved.session.target().is_remote();
    anyhow::ensure!(
        !args.foverride_conf || args.force,
        "--foverride-conf requires --force"
    );
    let recognition = Recognize {
        path: resolved.path.clone(),
        start_script: args.start_script,
        minecraft_version: args.minecraft_version,
        loader: args.loader,
        loader_version: args.loader_version,
    };
    recognition.validate()?;
    let manual = recognition.manual_metadata();
    let runtime = Runtime::new()?;
    let progress = RefCell::new(None);
    let (metadata, configuration) = match manual {
        Some(metadata) => (metadata, None),
        None => {
            let candidates = runtime.block_on(recognize(
                &resolved.session,
                recognition,
                &progress,
                remote,
            ))?;
            progress.borrow_mut().take();
            let index = if candidates.len() == 1 {
                0
            } else {
                let Some(index) = selector::select(&candidates, args.no_ui)? else {
                    tracing::info!("Server initialization cancelled before writing");
                    return Ok(());
                };
                index
            };
            tracing::debug!(
                index,
                candidates = candidates.len(),
                "Selected server launch snapshot"
            );
            let candidate = candidates.into_iter().nth(index).expect("selected candidate");
            (candidate.metadata, candidate.configuration)
        }
    };
    let initialization = Initialize {
        path: resolved.path,
        metadata,
        force: args.force,
        no_git: args.no_git,
        git: args.git,
        configuration,
        replace_conf: args.foverride_conf,
    };
    let info = runtime.block_on(register(
        &resolved.session,
        initialization,
        &progress,
        remote,
    ))?;
    crate::commands::execution::finish(&runtime, &resolved.session)?;
    progress.borrow_mut().take();
    write_info(&info, args.full_command)
}

fn create_remote(mut args: InitArgs, remote_path: String, ssh: SshArgs) -> Result<()> {
    anyhow::ensure!(ssh.is_requested(), "--remote requires global --ssh");
    let (command, executable) = ssh.settings()?;
    let runtime = Runtime::new()?;
    let caller_directory = std::fs::canonicalize(&args.path)
        .context("Remote workspace PATH must be an existing local directory")?;
    let surface = local_session()?;
    runtime.block_on(async {
        let handle = surface
            .start::<Validate>(ValidateInput {
                path: caller_directory.clone(),
                replace: args.replace,
            })
            .await
            .map_err(validate_failure)?;
        handle.finish().await.map_err(validate_failure)
    })?;
    let host_workspace = mcpm_backend::ResolvedWorkspace {
        path: PathBuf::from(&remote_path),
        connection: Some(ConnectionSpec {
            ssh: command.clone(),
            executable: executable.clone(),
            directory: Some(caller_directory),
        }),
        remote: None,
    };
    let host_session = crate::commands::execution::session_for_workspace(&host_workspace)?;
    let recognition = Recognize {
        path: PathBuf::from(&remote_path),
        start_script: args.start_script.take(),
        minecraft_version: args.minecraft_version.take(),
        loader: args.loader.take(),
        loader_version: args.loader_version.take(),
    };
    recognition.validate()?;
    let progress = RefCell::new(None);
    let (metadata, configuration) = match recognition.manual_metadata() {
        Some(metadata) => (metadata, None),
        None => {
            let candidates = runtime.block_on(recognize(&host_session, recognition, &progress, true))?;
            progress.borrow_mut().take();
            let index = if candidates.len() == 1 {
                0
            } else {
                let Some(index) = selector::select(&candidates, args.no_ui)? else {
                    return Ok(());
                };
                index
            };
            let candidate = candidates.into_iter().nth(index).expect("selected candidate");
            (candidate.metadata, candidate.configuration)
        }
    };
    let initialized = runtime.block_on(register(
        &host_session,
        Initialize {
            path: PathBuf::from(&remote_path),
            metadata,
            force: args.force,
            no_git: args.no_git,
            git: args.git,
            configuration,
            replace_conf: args.foverride_conf,
        },
        &progress,
        true,
    ))?;
    progress.borrow_mut().take();
    let registration = runtime.block_on(async {
        let handle = surface
            .start::<RemoteRegister>(RegisterInput {
                path: args.path.clone(),
                connection: ConnectionSpec {
                    ssh: command,
                    executable,
                    directory: None,
                },
                remote_path,
                replace: args.replace,
            })
            .await
            .map_err(surface_failure)?;
        handle.finish().await.map_err(surface_failure)
    });
    if let Err(error) = registration {
        return Err(error.context("Host server initialized successfully, but caller Remote workspace registration failed; host files were not rolled back. Re-run with --replace to register the placeholder or inspect the workspace and register it manually."));
    }
    crate::commands::execution::finish(&runtime, &host_session)?;
    write_info(&initialized, args.full_command)
}

fn surface_failure(failure: mcpm_exec::Failure<mcpm_backend::workspace::RegisterError>) -> anyhow::Error {
    match failure {
        mcpm_exec::Failure::Operation(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Dispatch(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Transport(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Cancelled => anyhow::anyhow!("workspace registration was cancelled"),
    }
}

fn validate_failure(failure: mcpm_exec::Failure<mcpm_backend::workspace::ValidateError>) -> anyhow::Error {
    match failure {
        mcpm_exec::Failure::Operation(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Dispatch(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Transport(error) => anyhow::Error::new(error),
        mcpm_exec::Failure::Cancelled => anyhow::anyhow!("Remote workspace validation was cancelled"),
    }
}
async fn recognize(
    session: &mcpm_backend::Session,
    request: Recognize,
    progress: &RefCell<Option<Progress>>,
    remote: bool,
) -> Result<Vec<Candidate>> {
    let mut handle = session
        .start::<Recognition>(request)
        .await
        .map_err(|error| operation_failure(error, remote))?;
    while let Some(event) = handle.next().await {
        let RecognizeEvent::Stage { phase } = event;
        advance(progress, phase);
    }
    handle
        .finish()
        .await
        .map_err(|error| operation_failure(error, remote))
}

async fn register(
    session: &mcpm_backend::Session,
    request: Initialize,
    progress: &RefCell<Option<Progress>>,
    remote: bool,
) -> Result<Initialized> {
    let mut handle = session
        .start::<Register>(request)
        .await
        .map_err(|error| operation_failure(error, remote))?;
    while let Some(event) = handle.next().await {
        let RegisterEvent::Stage { phase } = event;
        advance(progress, phase);
    }
    handle
        .finish()
        .await
        .map_err(|error| operation_failure(error, remote))
}

fn advance(progress: &RefCell<Option<Progress>>, phase: Phase) {
    let description = match phase {
        Phase::Recognizing => "Recognizing server launch candidates...",
        Phase::Validating => "Validating server initialization...",
        Phase::Writing => "Writing server configuration...",
    };
    // Drop the previous progress guard before attaching the next one.
    progress.borrow_mut().take();
    *progress.borrow_mut() = Some(Progress::new(description, "Initializing server"));
}

fn write_info(info: &Initialized, full_command: bool) -> Result<()> {
    if let Some(publication) = &info.publication {
        output::write_publication(&mut io::stderr().lock(), publication)?;
    }
    let mut stdout = io::stdout().lock();
    output::write_git(&mut stdout, info.git.as_ref())
        .context("failed to write Git initialization result")?;
    crate::display::server::write_snapshot(&mut stdout, &info.server, full_command, None)
        .context("failed to write initialized server information")?;
    output::write_success(&mut stdout).context("failed to write initialization success message")?;
    stdout
        .flush()
        .context("failed to flush initialized server information")
}

/// CLI choices come from the same loader registry used by recognition and initialization.
fn loader_parser() -> clap::builder::PossibleValuesParser {
    let mut loaders: Vec<_> = kako_mc_lib::local::modloader::BUILTIN_LOADERS
        .keys()
        .map(|id| id.as_str())
        .collect();
    loaders.push("vanilla");
    loaders.sort_unstable();
    clap::builder::PossibleValuesParser::new(loaders)
}
