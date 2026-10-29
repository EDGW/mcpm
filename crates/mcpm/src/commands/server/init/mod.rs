//! Server initialization frontend: complete arguments, or recognition followed by selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::server::{
    Candidate, Info, InfoInput, Initialized, Initialize, LocalImport, Phase, Recognize,
    RecognizeEvent, Recognition, Register, RegisterEvent,
};
use mcpm_backend::workspace::{RegisterInput, RemoteRegister};
use mcpm_backend::ConnectionSpec;

use super::target;
use crate::commands::execution::{
    default_executable, failure as operation_failure, local_session, Runtime,
};
use crate::progress::Progress;
use crate::ssh::SshArgs;

mod output;
mod selector;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: TUI (candidate selection), Console (--no-ui), Raw (--raw or redirected stdout). Console and Raw require an unambiguous candidate or complete manual arguments.\n\nTwo different SSH operations:\n  {server} --ssh COMMAND [--ssh-mcpm EXE] init <REMOTE_PATH>\n    Initialize a Local Server on the SSH host. The positional path is remote.\n  {server} --ssh COMMAND [--ssh-mcpm EXE] init <PATH> --remote <REMOTE_PATH>\n    Create a Remote workspace locally. PATH is local; --remote points to an existing remote Local Server.\n\nComplete --minecraft-version and --loader (plus --loader-version unless vanilla) initialize directly without reading scripts. Otherwise, recognition returns candidates before initialization.", server = crate::program::roots().server)
)]
pub(super) struct InitArgs {
    /// Existing server directory; with --ssh it is remote, but with --remote it is a local workspace.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Register PATH locally as a Remote workspace pointing to this existing remote Local Server.
    #[arg(long, value_name = "REMOTE_PATH", conflicts_with_all = ["start_script", "minecraft_version", "loader", "loader_version"])]
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
    /// Regenerate conf.toml, including active settings and comments; requires --force (local only).
    #[arg(long, requires = "force", conflicts_with = "remote")]
    foverride_conf: bool,
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
    let resolved = target::resolve(args.path, ssh)?;
    let remote = resolved.session.target().is_remote();
    anyhow::ensure!(
        !args.foverride_conf || !remote,
        "--foverride-conf is supported only for local configuration import"
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
        local: (!remote).then_some(LocalImport {
            configuration,
            replace_conf: args.foverride_conf,
        }),
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

fn create_remote(args: InitArgs, remote_path: String, ssh: SshArgs) -> Result<()> {
    anyhow::ensure!(ssh.is_requested(), "--remote requires global --ssh");
    let (command, executable) = ssh.settings()?;
    let directory = std::fs::canonicalize(&args.path)
        .context("Remote workspace PATH must be an existing local directory")?;
    let runtime = Runtime::new()?;
    let resolved = mcpm_backend::ResolvedWorkspace {
        path: args.path.clone(),
        connection: Some(ConnectionSpec {
            ssh: command.clone(),
            executable: executable.clone(),
            directory: Some(directory),
        }),
        remote: None,
    };
    let remote_session = crate::commands::execution::session_for_workspace(&resolved)?;
    let info = runtime.block_on(async {
        let handle = remote_session
            .start::<Info>(InfoInput {
                path: PathBuf::from(&remote_path),
                include_configuration: false,
            })
            .await
            .map_err(|error| operation_failure(error, true))?;
        handle
            .finish()
            .await
            .map_err(|error| operation_failure(error, true))
    })?;
    let surface = local_session()?;
    let registration = runtime.block_on(async {
        let handle = surface
            .start::<RemoteRegister>(RegisterInput {
                path: args.path,
                connection: ConnectionSpec {
                    ssh: command,
                    executable,
                    directory: None,
                },
                remote_path,
                force: args.force,
                no_git: args.no_git,
                git: args.git,
                default_executable: default_executable().to_owned(),
            })
            .await
            .map_err(surface_failure)?;
        handle.finish().await.map_err(surface_failure)
    })?;
    crate::commands::execution::finish(&runtime, &remote_session)?;
    let initialized = Initialized {
        server: info,
        git: registration.git,
        publication: None,
    };
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
