//! Server initialization frontend: direct complete arguments, or read-only recognition followed by local selection.

use std::{
    cell::RefCell,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;

use crate::{commands::execution::Runtime, progress::Progress, ssh::SshArgs};
use mcpm_backend::operations::server::init::{
    Init, InitEvent, Initialize, Initialized, LocalImport, Phase, Recognize, RecognizeEvent,
    RecognizeServer,
};
use mcpm_lib::local::mc::server::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader, ServerModLoader,
};

mod output;
mod presentation;
mod reference;
mod selector;

/// Builds initialization modes and their examples.
fn after_help() -> clap::builder::StyledStr {
    use super::after_help::Help;

    let mut help = Help::new();
    help.heading("Initialization modes");
    help.blank();
    help.line("  Recognize a startup script");
    help.line("    Omit identity options to import an existing Java launch.");
    help.line("    One candidate is used directly; for multiple candidates, select a card");
    help.line("    in TUI or enter a number in Console. Raw lists the candidates and fails.");
    help.line("    Example: mcpm init ./server --start-script run.sh");
    help.line("    Example: mcpm init -FIg ./server");
    help.blank();
    help.line("  Set the identity manually");
    help.line("    Supply the complete identity. This registers the server without");
    help.line("    generating launch.toml. Partial identity is rejected.");
    help.line("    Example: mcpm init ./server --minecraft-version 1.21.1 --loader vanilla");
    help.blank();
    help.line("  Register a Remote workspace");
    help.line("    With --remote, PATH is local and REMOTE_PATH names an already registered");
    help.line("    server on the SSH host.");
    help.line("    Example: mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft");
    help.blank();
    help.heading("Server location");
    help.blank();
    help.line("  Without --remote, an explicit --ssh makes PATH a remote server directory.");
    help.line("  Otherwise, PATH selects a local directory or follows its saved connection.");
    help.finish()
}

#[derive(Args, Debug)]
#[command(
    after_help = after_help(),
    group(clap::ArgGroup::new("force_write").args(["force", "force_all"]).multiple(true))
)]
pub(super) struct InitArgs {
    /// Existing server or workspace directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Register a local workspace pointing to this existing remote Local Server; requires --ssh.
    #[arg(long, value_name = "REMOTE_PATH", help_heading = "Remote workspace", conflicts_with_all = ["start_script", "minecraft_version", "loader", "loader_version"])]
    remote: Option<String>,
    /// Scan this server-relative script. Default: run.sh then start.sh (run.bat then start.bat on Windows).
    #[arg(long, value_name = "FILE", help_heading = "Script recognition", conflicts_with_all = ["minecraft_version", "loader", "loader_version"])]
    start_script: Option<PathBuf>,
    /// Warn and skip unsupported statements without changing the known working directory or variables.
    #[arg(
        short = 'I',
        long,
        help_heading = "Script recognition",
        conflicts_with_all = ["remote", "minecraft_version", "loader", "loader_version"]
    )]
    ignore_unknown_vars: bool,
    /// Minecraft version for manual initialization.
    #[arg(
        long,
        alias = "mc-version",
        value_name = "VERSION",
        help_heading = "Manual identity"
    )]
    minecraft_version: Option<String>,
    /// Loader type for manual initialization.
    #[arg(long, value_parser = loader_parser(), help_heading = "Manual identity")]
    loader: Option<String>,
    /// Required for a modded server; omit for vanilla.
    #[arg(long, value_name = "VERSION", help_heading = "Manual identity")]
    loader_version: Option<String>,
    /// Replace the existing server or Remote workspace registration.
    #[arg(short = 'f', long, help_heading = "Write options")]
    force: bool,
    /// Replace registration and regenerate conf.toml (same as --force --foverride-conf).
    #[arg(short = 'F', conflicts_with = "remote", help_heading = "Write options")]
    force_all: bool,
    /// Regenerate conf.toml, including active settings and comments; requires --force.
    #[arg(
        long,
        requires = "force_write",
        conflicts_with = "remote",
        help_heading = "Write options"
    )]
    foverride_conf: bool,
    /// Skip Git discovery and initialization on the actual server host.
    #[arg(short = 'G', long, help_heading = "Git options")]
    no_git: bool,
    /// Require a Git repository in the server directory itself.
    #[arg(
        short = 'g',
        long,
        conflicts_with = "no_git",
        help_heading = "Git options"
    )]
    git: bool,
    /// Use the numbered Console list for candidate selection.
    #[arg(long, help_heading = "Script recognition")]
    no_ui: bool,
    /// Show the complete startup command generated from launch.toml and conf.toml.
    #[arg(long, help_heading = "Result options")]
    full_command: bool,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: InitArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(run_async(args, ssh))
}

async fn run_async(args: InitArgs, ssh: SshArgs) -> Result<()> {
    let force = args.force || args.force_all;
    if let Some(remote_path) = args.remote {
        let info = reference::create(
            args.path,
            remote_path,
            ssh,
            force,
            args.no_git,
            args.git,
            args.full_command,
        )
        .await?;
        return write_info(&info, args.full_command);
    }
    let manual = manual_metadata(args.minecraft_version, args.loader, args.loader_version)?;
    let (executor, path, _) = ssh.create_executor(Some(args.path)).await?;
    let path = path.context("server initialization requires a path")?;
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
    let (metadata, configuration) = match manual {
        Some(metadata) => (metadata, None),
        None => {
            let mut stream = executor
                .call::<RecognizeServer>(Recognize {
                    path: path.clone(),
                    start_script: args.start_script,
                    ignore_unknown_vars: args.ignore_unknown_vars,
                })
                .await?;
            let mut result = None;
            while let Some(event) = stream.next().await {
                match event? {
                    RecognizeEvent::Stage(stage) => phase(stage),
                    RecognizeEvent::Warning(warning) => {
                        progress.borrow_mut().take();
                        output::write_skipped_statement(&mut io::stderr().lock(), &warning)?;
                    }
                    RecognizeEvent::Candidates(candidates) => result = Some(candidates),
                }
            }
            progress.borrow_mut().take();
            let mut candidates = result.context("recognition stream ended without candidates")?;
            let index = if candidates.len() == 1 {
                0
            } else {
                if !crate::terminal::stdout_is_tty() {
                    return Err(presentation::ambiguous(&candidates));
                }
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
            path: path.clone(),
            metadata,
            force,
            no_git: args.no_git,
            git: args.git,
            local: Some(LocalImport {
                configuration,
                replace_conf: args.foverride_conf || args.force_all,
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
    let mut result = result.context("initialization stream ended without its result")?;
    if args.full_command {
        result.server = executor
            .call::<mcpm_backend::operations::server::info::Info>(
                mcpm_backend::operations::server::info::InfoInput {
                    path,
                    full_command: true,
                    allow_legacy: false,
                },
            )
            .await?;
    }
    write_info(&result, args.full_command)
}

fn manual_metadata(
    minecraft_version: Option<String>,
    loader: Option<String>,
    loader_version: Option<String>,
) -> Result<Option<MinecraftServerMetadata>> {
    let (minecraft_version, loader) = match (minecraft_version, loader, &loader_version) {
        (None, None, None) => return Ok(None),
        (Some(minecraft_version), Some(loader), _) => (minecraft_version, loader),
        _ => bail!("manual initialization requires both --minecraft-version and --loader"),
    };
    ensure!(
        !minecraft_version.trim().is_empty()
            && loader_version
                .as_ref()
                .is_none_or(|version| !version.trim().is_empty()),
        "version options must not be empty"
    );
    let loader = match (loader.as_str(), loader_version) {
        ("vanilla", None) => ServerLoader::Vanilla,
        ("vanilla", Some(_)) => bail!("vanilla does not have a loader version"),
        (_, Some(version)) => ServerLoader::Modded(ServerModLoader {
            loader_type: loader,
            version,
        }),
        (_, None) => bail!("manual initialization with {loader} requires --loader-version"),
    };
    Ok(Some(MinecraftServerMetadata {
        version: SERVER_METADATA_VERSION,
        minecraft_version,
        loader,
    }))
}

fn write_info(info: &Initialized, full_command: bool) -> Result<()> {
    if let Some(publication) = &info.publication {
        output::write_publication(&mut io::stderr().lock(), publication)?;
    }
    let mut stdout = io::stdout().lock();
    super::git_output::write_git(&mut stdout, info.git.as_ref())
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
