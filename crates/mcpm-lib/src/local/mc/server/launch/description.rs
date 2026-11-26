//! Read-only command composition shared by launch preparation and info output.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::super::{
    authlib_injector,
    config::{self, LaunchConfig, Platform, ServerConfig},
};
use crate::local::java::command::JavaCommand;

/// Configured argv, without resolving Java, probing capabilities, or preparing downloads.
#[derive(Debug)]
pub struct CommandDescription {
    pub executable: String,
    pub directory: PathBuf,
    pub arguments: Vec<String>,
    pub windows: bool,
}

/// Missing configuration has no startup command; existing invalid configuration remains an error.
pub fn describe(root: &Path) -> Result<Option<CommandDescription>> {
    let root = std::fs::canonicalize(root).context("failed to resolve server directory")?;
    let _lock = config::ServerLock::acquire_read(&root)?;
    let files = config::configuration_files(&root)?;
    if !files.launch || !files.conf {
        return Ok(None);
    }
    let (launch, mut conf) = config::read(&root, &config::ConfigurationOverrides::default())?;
    let executable = conf.java.executable.take().unwrap_or_else(|| "java".into());
    let directory = root.join(launch.process.working_directory.as_deref().unwrap_or("."));
    let windows = launch.platform.unwrap_or_else(Platform::current) == Platform::Windows;
    let command = compose_command(&root, launch, conf)?;
    Ok(Some(CommandDescription {
        executable,
        directory,
        windows,
        arguments: command.arguments(),
    }))
}

/// Composes validated launch and server configurations into one ordered [`JavaCommand`].
pub(crate) fn compose_command(
    root: &Path,
    mut launch: LaunchConfig,
    conf: ServerConfig,
) -> Result<JavaCommand> {
    let jvm = base_jvm(&mut launch, &conf)?;
    finish_command(root, launch, conf, jvm)
}

pub(super) fn base_jvm(launch: &mut LaunchConfig, conf: &ServerConfig) -> Result<Vec<String>> {
    let mut jvm = std::mem::take(&mut launch.jvm.extra_arguments);
    jvm.extend(conf.jvm_arguments()?);
    Ok(jvm)
}

/// Configuration owns each argument region and the order is shared by inspection and execution.
pub(super) fn finish_command(
    root: &Path,
    launch: LaunchConfig,
    conf: ServerConfig,
    mut jvm: Vec<String>,
) -> Result<JavaCommand> {
    if let Some(auth) = &conf.authlib_injector {
        let path = auth
            .manual_jar()
            .map(|path| root.join(path))
            .unwrap_or_else(|| authlib_injector::managed_path(root));
        jvm.extend(auth.arguments(&path)?);
    }
    let mut application = launch.application.extra_arguments;
    application.extend(conf.server_arguments());
    application.extend(conf.application.extra_arguments);
    Ok(JavaCommand {
        jvm,
        entry: launch.entry,
        application,
    })
}
