//! Host-routed launch preparation followed by independent shim launch and optional attachment.

use crate::{commands::CommandOutcome, ssh::SshArgs};
use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use mcpm_backend::operations::server::shim;
use mcpm_lib::local::mc::server::authlib_injector::JarPolicy;
use mcpm_lib::local::mc::server::config::ConfigurationOverrides;
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(
    after_help = super::after_help::launch()
)]
pub(super) struct LaunchArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[command(flatten)]
    overrides: OverrideArgs,
    #[command(flatten)]
    auth: AuthArgs,
    #[command(flatten)]
    execution: ExecutionArgs,
    #[command(flatten)]
    attachment: AttachmentArgs,
}

impl LaunchArgs {
    fn configuration_overrides(&self) -> Result<ConfigurationOverrides> {
        let mut overrides = self.overrides.build()?;
        self.auth.apply(&mut overrides)?;
        Ok(overrides)
    }
}

/// The registered server directory that launch preparation targets.
#[derive(Debug, Args)]
struct TargetArgs {
    /// Registered server directory on the selected Host.
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
}

/// Typed shortcuts for conf fields; application order follows declaration.
#[derive(Debug, Args)]
#[command(next_help_heading = "Override options")]
struct OverrideArgs {
    /// Override a conf field for this launch. Lists: = or + appends; := clears and sets.
    #[arg(short = 'O', long = "override", value_name = "KEY=VALUE")]
    overrides: Vec<String>,
    /// Override java.executable for this launch.
    #[arg(short = 'j', long, value_name = "EXECUTABLE")]
    java: Option<String>,
    /// Override maximum heap, optionally also initial heap (INITIAL:MAXIMUM).
    #[arg(short = 'm', long, value_name = "[INITIAL:]MAXIMUM")]
    memory: Option<String>,
    /// Override jvm.gc.collector for this launch.
    #[arg(short = 'G', long, value_name = "COLLECTOR")]
    gc: Option<String>,
    /// Override server.port for this launch.
    #[arg(short = 'p', long, value_name = "PORT")]
    port: Option<u16>,
    /// Override server.world for this launch.
    #[arg(long, value_name = "DIRECTORY")]
    world: Option<String>,
    /// Append one JVM argument after -O list changes. May be specified once.
    #[arg(short = 'E', long, value_name = "ARGUMENT", allow_hyphen_values = true)]
    extra: Option<String>,
}

impl OverrideArgs {
    fn build(&self) -> Result<ConfigurationOverrides> {
        let mut overrides = ConfigurationOverrides::default();
        for assignment in &self.overrides {
            overrides.parse(assignment)?;
        }
        if let Some(java) = &self.java {
            overrides.set("java.executable", java)?;
        }
        if let Some(memory) = &self.memory {
            if let Some((initial, maximum)) = memory.split_once(':') {
                ensure!(
                    !initial.is_empty() && !maximum.is_empty() && !maximum.contains(':'),
                    "memory must be MAXIMUM or INITIAL:MAXIMUM"
                );
                overrides.set("jvm.heap.initial", initial)?;
                overrides.set("jvm.heap.maximum", maximum)?;
            } else {
                overrides.set("jvm.heap.maximum", memory)?;
            }
        }
        if let Some(gc) = &self.gc {
            overrides.set("jvm.gc.collector", gc)?;
        }
        if let Some(port) = self.port {
            overrides.set("server.port", port)?;
        }
        if let Some(world) = &self.world {
            overrides.set("server.world", world)?;
        }
        if let Some(extra) = &self.extra {
            overrides.set("jvm.extraArguments", vec![extra])?;
        }
        Ok(overrides)
    }
}

/// Managed authentication endpoint and JAR lifecycle.
#[derive(Debug, Args)]
#[command(next_help_heading = "Authentication")]
struct AuthArgs {
    /// Override authlib-injector.apiUrl for this launch; enables managed authentication if absent.
    #[arg(short = 'A', long, value_name = "API_URL")]
    auth: Option<String>,
    /// Managed authentication JAR: auto repairs if needed, local forbids downloads, refresh reinstalls latest.
    #[arg(long, value_enum, value_name = "POLICY")]
    auth_jar_policy: Option<AuthJarPolicy>,
}

impl AuthArgs {
    fn apply(&self, overrides: &mut ConfigurationOverrides) -> Result<()> {
        if let Some(auth) = &self.auth {
            overrides.set("authlib-injector.apiUrl", auth)?;
        }
        Ok(())
    }
}

/// Launch transport and whether the command attaches after Java starts.
#[derive(Debug, Args)]
#[command(next_help_heading = "Launch Panel options")]
struct ExecutionArgs {
    /// Command transport for this immutable Java instance.
    #[arg(short = 'c', long, value_enum, default_value = "auto")]
    communication: Communication,
    /// Return once the shim owns Java, without attaching.
    #[arg(short = 'D', long)]
    detach: bool,
}

/// Inline attachment presentation; ignored when --detach is used.
#[derive(Debug, Args)]
#[command(next_help_heading = "Mode options")]
struct AttachmentArgs {
    /// Disable the inline interface and use Console/Raw output.
    #[arg(long)]
    no_ui: bool,
    /// Initial inline input height, adjustable while attached.
    #[arg(long, default_value_t = super::attach::DEFAULT_INPUT_HEIGHT, value_parser = clap::value_parser!(u16).range(3..), help = format!("Initial inline input height; {}+Up/Down adjusts it while attached", crate::display::server::attach::modifier_name()))]
    input_height: u16,
}

/// CLI spellings adapt to the backend policy; parsing never implements file lifecycle rules.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum AuthJarPolicy {
    Auto,
    Local,
    Refresh,
}

impl From<AuthJarPolicy> for JarPolicy {
    fn from(value: AuthJarPolicy) -> Self {
        match value {
            AuthJarPolicy::Auto => Self::Auto,
            AuthJarPolicy::Local => Self::Local,
            AuthJarPolicy::Refresh => Self::Refresh,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Communication {
    Auto,
    Rcon,
    Stdin,
}
impl From<Communication> for shim::Communication {
    fn from(value: Communication) -> Self {
        match value {
            Communication::Auto => Self::Auto,
            Communication::Rcon => Self::Rcon,
            Communication::Stdin => Self::Stdin,
        }
    }
}

pub(super) fn run(args: LaunchArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let overrides = args.configuration_overrides()?;
    let runtime = crate::commands::execution::Runtime::new()?;
    runtime.block_on(async move {
        let (executor, server, _) = ssh.create_executor(Some(args.target.server)).await?;
        let mut events = executor.call::<shim::Prepare>(shim::PrepareInput {
            server: server.context("launch requires a server path")?, overrides,
            auth_jar_policy: args.auth.auth_jar_policy.map(Into::into),
        }).await?;
        let mut progress = crate::display::server::launch::LaunchProgress::default();
        let mut prepared = None;
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);
        loop {
            tokio::select! {
                signal = &mut interrupt => { signal?; return Ok(CommandOutcome::Exited(130)); }
                event = events.next() => match event {
                    Some(Ok(shim::PrepareEvent::Progress { message, bytes })) => progress.host_event(&message, bytes),
                    Some(Ok(shim::PrepareEvent::Prepared(plan))) => prepared = Some(plan),
                    Some(Err(error)) => return Err(error),
                    None => break,
                }
            }
        }
        let prepared = prepared.context("launch preparation ended without a plan")?;
        progress.starting();
        drop(progress);
        let launched = executor.call::<shim::Launch>(shim::LaunchInput {
            server: prepared.server, plan: prepared.plan, communication: args.execution.communication.into(),
            launch_request_id: uuid::Uuid::new_v4().to_string(),
        }).await?;
        if args.execution.detach {
            println!("{}", launched.reference.run_id);
            return Ok(CommandOutcome::Completed);
        }
        super::attach::flow(executor, launched.reference, Some(launched.initial_cursor), None, args.attachment.no_ui, args.attachment.input_height).await
    })
}
