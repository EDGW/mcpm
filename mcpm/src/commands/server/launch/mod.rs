//! Local-only server launch routing; the backend owns configuration, argv and child lifetime.

use crate::{commands::CommandOutcome, ssh::SshArgs};
use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::local::mc::server::config::ConfigurationOverrides;
use mcpm_backend::local::mc::server::launch::{self, ForegroundStreams};
use std::{path::PathBuf, sync::mpsc};

mod signals;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw. Both inherit the Java process's stdin, stdout and stderr without converting game output. Reads launch.toml and conf.toml from the registered local server root. Runs in the foreground and returns Java's exit status. SSH and Remote workspaces are not supported."
)]
pub(super) struct LaunchArgs {
    /// Registered local server directory containing launch.toml and conf.toml.
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    /// Override a conf field for this launch. Lists: = or + appends; := clears and sets.
    #[arg(short = 'O', long = "override", value_name = "KEY=VALUE")]
    overrides: Vec<String>,
    /// Override java.executable for this launch.
    #[arg(long, value_name = "EXECUTABLE")]
    java: Option<String>,
    /// Override maximum heap, optionally also initial heap (INITIAL:MAXIMUM).
    #[arg(short = 'm', long, value_name = "[INITIAL:]MAXIMUM")]
    memory: Option<String>,
    /// Override jvm.gc.collector for this launch.
    #[arg(short = 'G', long, value_name = "COLLECTOR")]
    gc: Option<String>,
    /// Override server.port for this launch.
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,
    /// Override server.world for this launch.
    #[arg(long, value_name = "DIRECTORY")]
    world: Option<String>,
    /// Append one JVM argument after -O list changes. May be specified once.
    #[arg(short = 'E', long, value_name = "ARGUMENT", allow_hyphen_values = true)]
    extra: Option<String>,
}

impl LaunchArgs {
    fn configuration_overrides(&self) -> Result<ConfigurationOverrides> {
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

pub(super) fn run(args: LaunchArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    ensure!(
        !ssh.is_requested(),
        "launch supports only local servers; SSH is not supported"
    );
    super::target::require_supported_local(&args.server, "launch")?;
    let overrides = args.configuration_overrides()?;
    let (sender, stop) = mpsc::channel();
    let _signals = signals::Handler::install(sender)?;
    let plan = launch::prepare_with_overrides(&args.server, &overrides)?;
    if let Ok(request) = stop.try_recv() {
        return Ok(CommandOutcome::Exited(match request {
            launch::StopRequest::Interrupt | launch::StopRequest::ConsoleInterrupt => 130,
            launch::StopRequest::Terminate => 143,
        }));
    }
    let result = launch::execute(plan, ForegroundStreams::Inherit, &stop)?;
    Ok(CommandOutcome::Exited(result.shell_code()))
}
