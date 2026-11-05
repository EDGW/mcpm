//! Server initialization frontend: direct complete arguments, or read-only recognition followed by local selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{commands::execution::Runtime, progress::Progress, ssh::SshArgs};
use mcpm_backend::operations::server::init::{
    Init, InitEvent, Initialize, Initialized, LocalImport, Phase, Recognize, RecognizeEvent,
    RecognizeServer,
};

mod output;
mod reference;
mod selector;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: TUI (local candidate selection), Console (--no-ui), Raw (--raw or redirected stdout). Explicit --ssh selects the data-source host; otherwise workspace registration selects Local or Remote execution. Complete version/loader arguments initialize directly. Incomplete arguments recognize candidates before local selection and initialization. --remote REMOTE_PATH registers a local Remote workspace and requires --ssh; the remote directory must already be a Local Server."
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
    /// Regenerate conf.toml, including active settings and comments; requires --force.
    #[arg(long, requires = "force", conflicts_with = "remote")]
    foverride_conf: bool,
    /// Skip Git discovery and initialization on the actual server host.
    #[arg(long)]
    no_git: bool,
    /// Require a repository in the actual server directory, including the remote Host with --remote; never in the local stub.
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
pub(super) fn run(args: InitArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(run_async(args, ssh))
}

async fn run_async(args: InitArgs, ssh: SshArgs) -> Result<()> {
    if let Some(remote_path) = args.remote {
        let info = reference::create(
            args.path,
            remote_path,
            ssh,
            args.force,
            args.no_git,
            args.git,
        )
        .await?;
        return write_info(&info, args.full_command);
    }
    let (executor, path, _) = ssh.create_executor(Some(args.path)).await?;
    let path = path.context("server initialization requires a path")?;
    let recognition = Recognize {
        path,
        start_script: args.start_script,
        minecraft_version: args.minecraft_version,
        loader: args.loader,
        loader_version: args.loader_version,
    };
    let path = recognition.path.clone();
    let progress = RefCell::new(None);
    let phase = |phase| {
        let description = match phase {
            Phase::Recognizing => "Recognizing server launch candidates...",
            Phase::Validating => "Validating server initialization...",
            Phase::Writing => "Writing server configuration...",
        };
        progress.borrow_mut().take();
        *progress.borrow_mut() = Some(Progress::new(description, "Initializing server"));
    };
    let (metadata, configuration) = match recognition.manual_metadata()? {
        Some(metadata) => (metadata, None),
        None => {
            let mut stream = executor.call::<RecognizeServer>(recognition).await?;
            let mut result = None;
            while let Some(event) = stream.next().await {
                match event? {
                    RecognizeEvent::Stage(stage) => phase(stage),
                    RecognizeEvent::Candidates(candidates) => result = Some(candidates),
                }
            }
            progress.borrow_mut().take();
            let mut candidates = result.context("recognition stream ended without candidates")?;
            let index = if candidates.len() == 1 {
                0
            } else {
                let Some(index) = selector::select(&candidates, args.no_ui)? else {
                    return Ok(());
                };
                index
            };
            let candidate = candidates.swap_remove(index);
            (candidate.metadata, candidate.configuration)
        }
    };
    let mut stream = executor
        .call::<Init>(Initialize {
            path,
            metadata,
            force: args.force,
            no_git: args.no_git,
            git: args.git,
            local: Some(LocalImport {
                configuration,
                replace_conf: args.foverride_conf,
            }),
        })
        .await?;
    let mut result = None;
    while let Some(event) = stream.next().await {
        match event? {
            InitEvent::Stage(stage) => phase(stage),
            InitEvent::Initialized(info) => result = Some(info),
        }
    }
    progress.borrow_mut().take();
    write_info(
        &result.context("initialization stream ended without its result")?,
        args.full_command,
    )
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
    let mut loaders: Vec<_> = mcpm_lib::local::modloader::BUILTIN_LOADERS
        .keys()
        .map(|id| id.as_str())
        .collect();
    loaders.push("vanilla");
    loaders.sort_unstable();
    clap::builder::PossibleValuesParser::new(loaders)
}
