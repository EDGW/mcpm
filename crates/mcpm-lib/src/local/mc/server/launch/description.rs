//! Read-only command composition shared by launch preparation and info output.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

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
    let _lock = config::ConfigurationLock::acquire(&root, false)?;
    let files = config::configuration_files(&root)?;
    if !files.launch || !files.conf {
        return Ok(None);
    }
    let (mut launch, mut conf) = config::read(&root, &config::ConfigurationOverrides::default())?;
    let executable = conf.java.executable.take().unwrap_or_else(|| "java".into());
    let directory = root.join(launch.process.working_directory.as_deref().unwrap_or("."));
    let windows = launch.platform.unwrap_or_else(Platform::current) == Platform::Windows;
    let jvm = base_jvm(&mut launch, &conf)?;
    let arguments = finish_arguments(&root, launch, conf, jvm)?;
    Ok(Some(CommandDescription {
        executable,
        directory,
        windows,
        arguments,
    }))
}

pub(super) fn base_jvm(launch: &mut LaunchConfig, conf: &ServerConfig) -> Result<Vec<String>> {
    let mut jvm = std::mem::take(&mut launch.jvm.extra_arguments);
    jvm.extend(conf.jvm_arguments()?);
    Ok(jvm)
}

/// Configuration owns each argument region and the order is shared by inspection and execution.
pub(super) fn finish_arguments(
    root: &Path,
    launch: LaunchConfig,
    conf: ServerConfig,
    mut jvm: Vec<String>,
) -> Result<Vec<String>> {
    if let Some(auth) = &conf.authlib_injector {
        let path = auth
            .manual_jar()
            .map(|path| root.join(path))
            .unwrap_or_else(|| authlib_injector::managed_path(root));
        jvm.extend(auth.arguments(&path)?);
    }
    let arguments: Vec<_> = jvm
        .into_iter()
        .chain(launch.entry.arguments())
        .chain(launch.application.extra_arguments)
        .chain(conf.server_arguments())
        .chain(conf.application.extra_arguments)
        .collect();
    let parsed = JavaCommand::parse(&arguments)?;
    ensure!(
        parsed.entry == launch.entry,
        "assembled Java entry differs from declared launch entry"
    );
    Ok(arguments)
}
