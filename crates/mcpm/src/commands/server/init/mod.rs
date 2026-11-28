//! Server initialization frontend: direct complete arguments, or read-only recognition followed by local selection.

use std::{
    cell::RefCell,
    fmt,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;

use crate::{commands::execution::Runtime, progress::Progress, ssh::SshArgs};
use mcpm_backend::operations::server::init::{
    Init, InitEvent, InitRconProbe, InitRconRequest, Initialize, Initialized, LocalImport, Phase,
    ProbeInitRcon, ProbeRcon, Recognize, RecognizeEvent, RecognizeServer,
};
use mcpm_lib::local::mc::server::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader, ServerModLoader, properties,
};

mod manual;
mod output;
mod presentation;
mod rcon_prompt;
mod reference;
mod selector;
mod terminal_input;

/// Builds initialization modes and their examples.
fn after_help() -> clap::builder::StyledStr {
    use super::after_help::Help;

    let mut help = Help::new();
    help.heading("Initialization modes");
    help.blank();
    help.line("  Recognize a startup script");
    help.line("    Omit identity options to import an existing Java launch.");
    help.line("    One candidate is used directly. Console lists multiple candidates with");
    help.line("    manual initialization commands; enter a number to import a launch.");
    help.line("    Raw lists the candidates and fails (--raw or redirected stdout).");
    help.line("    Example: mcpm init ./server --start-script run.sh");
    help.line("    Example: mcpm init -FIg ./server");
    help.blank();
    help.line("  Set the identity manually");
    help.line("    Supply the complete identity. Add --entry to write launch.toml and");
    help.line("    --conf to set fields in the generated conf.toml.");
    help.line("    Example: mcpm init ./server --minecraft-version 1.21.1 --loader vanilla");
    help.line("    Example: mcpm init . --minecraft-version 1.21.1 --loader vanilla");
    help.line("      --entry jar@server.jar --conf jvm.heap.maximum=4G");
    help.blank();
    help.line("  Register a Remote workspace");
    help.line("    With --remote, PATH is local and REMOTE_PATH names an already registered");
    help.line("    server on the SSH host.");
    help.line("    Example: mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft");
    help.blank();
    help.heading("RCON configuration");
    help.blank();
    help.line("  Console mode prompts to enable RCON for Minecraft 1.9-pre4+ whenever");
    help.line("  server.properties lacks a complete enabled RCON setup (default: Y,");
    help.line("  port default: 25575, missing password generates a UUID).");
    help.line(
        "  Raw mode leaves RCON disabled unless --rcon / --rcon-port / --rcon-password is passed.",
    );
    help.line("  Explicit --rcon* options skip prompting, override matching fields, and");
    help.line("  require both rcon.port and rcon.password to be specified or present.");
    help.blank();
    help.heading("Server location");
    help.blank();
    help.line("  Without --remote, an explicit --ssh makes PATH a remote server directory.");
    help.line("  Otherwise, PATH selects a local directory or follows its saved connection.");
    help.finish()
}

#[derive(Args)]
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
    #[command(flatten)]
    manual: manual::ManualArgs,
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
    /// Use --conf only when generating a new conf.toml; keep existing settings unchanged.
    #[arg(long, requires = "conf", conflicts_with_all = ["foverride_conf", "force_all"], help_heading = "Write options")]
    preserve_conf: bool,
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
    /// Enable RCON in server.properties without prompting; requires complete config or overrides.
    #[arg(long, conflicts_with = "remote", help_heading = "RCON options")]
    rcon: bool,
    /// Set and override rcon.port in server.properties; implies --rcon.
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(u16).range(1..),
        conflicts_with = "remote",
        help_heading = "RCON options"
    )]
    rcon_port: Option<u16>,
    /// Set and override rcon.password in server.properties; implies --rcon.
    #[arg(
        long,
        value_name = "PASSWORD",
        conflicts_with = "remote",
        help_heading = "RCON options"
    )]
    rcon_password: Option<String>,
    /// Show the complete startup command generated from launch.toml and conf.toml.
    #[arg(long, help_heading = "Result options")]
    full_command: bool,
}

impl fmt::Debug for InitArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InitArgs")
            .field("path", &self.path)
            .field("remote", &self.remote)
            .field("start_script", &self.start_script)
            .field("ignore_unknown_vars", &self.ignore_unknown_vars)
            .field("minecraft_version", &self.minecraft_version)
            .field("loader", &self.loader)
            .field("loader_version", &self.loader_version)
            .field("manual", &self.manual)
            .field("force", &self.force)
            .field("force_all", &self.force_all)
            .field("foverride_conf", &self.foverride_conf)
            .field("preserve_conf", &self.preserve_conf)
            .field("no_git", &self.no_git)
            .field("git", &self.git)
            .field("rcon", &self.rcon)
            .field("rcon_port", &self.rcon_port)
            .field(
                "rcon_password",
                &self.rcon_password.as_ref().map(|_| "<redacted>"),
            )
            .field("full_command", &self.full_command)
            .finish()
    }
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: InitArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(run_async(args, ssh))
}

async fn run_async(args: InitArgs, ssh: SshArgs) -> Result<()> {
    let force = args.force || args.force_all;
    let replace_conf = args.foverride_conf || args.force_all;
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
    let (manual_configuration, conf_overrides) = args.manual.build()?;
    let invoked_path =
        (manual.is_none() && crate::terminal::stdout_is_tty()).then(|| args.path.clone());
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
        Some(metadata) => (metadata, manual_configuration),
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
                let Some(invoked_path) = invoked_path else {
                    return Err(presentation::ambiguous(&candidates));
                };
                let mut command = crate::program::roots().server.command("init");
                let copyable = ssh.explicit_arguments().all(|(name, value)| {
                    presentation::append_option(&mut command, name, value).is_some()
                });
                for (name, enabled) in [
                    (" --force", force),
                    (" --foverride-conf", replace_conf),
                    (" --no-git", args.no_git),
                    (" --git", args.git),
                    (" --full-command", args.full_command),
                ] {
                    if enabled {
                        command.push_str(name);
                    }
                }
                let Some(index) = selector::select(
                    &candidates,
                    copyable.then_some(command.as_str()),
                    &invoked_path,
                    replace_conf,
                )?
                else {
                    return Ok(());
                };
                index
            };
            let candidate = candidates.swap_remove(index);
            (candidate.metadata, candidate.configuration)
        }
    };
    let explicit_rcon = args.rcon || args.rcon_port.is_some() || args.rcon_password.is_some();
    let rcon = if explicit_rcon {
        Some(InitRconRequest::Explicit {
            port: args.rcon_port,
            password: args.rcon_password,
        })
    } else if crate::terminal::stdout_is_tty()
        && properties::supports_rcon_properties(&metadata.minecraft_version)
    {
        match executor
            .call::<ProbeInitRcon>(ProbeRcon {
                path: path.clone(),
                launch: configuration.as_ref().map(|draft| draft.launch.clone()),
                replace_conf,
                conf_overrides: conf_overrides.clone(),
                preserve_conf: args.preserve_conf,
            })
            .await
        {
            Ok(InitRconProbe::AlreadyComplete) => None,
            Ok(InitRconProbe::ParseFailed { reason }) => {
                output::write_rcon_warning(
                    &mut io::stderr().lock(),
                    &format!("Failed to parse server.properties ({reason}); skipping RCON setup."),
                )?;
                None
            }
            Ok(InitRconProbe::NeedsPrompt {
                currently_enabled,
                existing_port,
                has_password,
                invalid_port,
            }) => rcon_prompt::prompt(
                currently_enabled,
                existing_port,
                has_password,
                invalid_port.as_deref(),
            )?,
            Err(error) => {
                output::write_rcon_warning(
                    &mut io::stderr().lock(),
                    &format!(
                        "Failed to inspect server.properties ({error:#}); skipping RCON setup."
                    ),
                )?;
                None
            }
        }
    } else {
        None
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
                replace_conf,
                conf_overrides,
                preserve_conf: args.preserve_conf,
                rcon,
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
    let launch_missing = info
        .server
        .configuration
        .as_ref()
        .is_some_and(|configuration| !configuration.launch);
    output::write_success(&mut stdout, launch_missing)
        .context("failed to write initialization success output")?;
    stdout
        .flush()
        .context("failed to flush initialized server information")?;
    drop(stdout);
    if let Some(outcome) = &info.rcon_outcome {
        output::write_rcon_outcome(&mut io::stderr().lock(), outcome)
            .context("failed to write RCON initialization outcome")?;
    }
    Ok(())
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
