//! Creates the single validated local Java execution plan from server-root configuration.

use crate::local::{
    java::{capabilities, command},
    mc::server::{
        MinecraftServer,
        config::{self, Platform},
    },
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// Immutable execution input; only backend preparation can construct it.
#[derive(Debug)]
pub struct LaunchPlan {
    pub(super) executable: PathBuf,
    pub(super) directory: PathBuf,
    pub(super) environment: BTreeMap<OsString, OsString>,
    pub(super) arguments: Vec<String>,
    pub(super) argument_file: bool,
}

impl LaunchPlan {
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
    }
}

/// Reads configuration under a short shared lock, then resolves the Java executable.
/// No source scripts, legacy start snapshots or server.properties are consulted.
pub fn prepare(root: &Path) -> Result<LaunchPlan> {
    prepare_with_overrides(root, &config::ConfigurationOverrides::default())
}

/// Builds a plan from the effective launch-only configuration without rewriting either file.
pub fn prepare_with_overrides(
    root: &Path,
    overrides: &config::ConfigurationOverrides,
) -> Result<LaunchPlan> {
    let root = std::fs::canonicalize(root).context("failed to resolve local server root")?;
    let (launch, conf) = {
        let _lock = config::ConfigurationLock::acquire(&root, false)?;
        MinecraftServer::inspect(&root)?;
        let (launch, conf) = config::read(&root, overrides)?;
        (launch, conf)
    };
    ensure!(
        launch
            .platform
            .is_none_or(|platform| platform == Platform::current()),
        "launch.toml targets a different platform; explicitly reimport a native start script"
    );
    let directory = std::fs::canonicalize(
        root.join(launch.process.working_directory.as_deref().unwrap_or(".")),
    )
    .context("failed to resolve Java working directory")?;
    ensure!(
        directory.is_dir(),
        "Java working directory is not a directory"
    );
    let mut environment: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in &launch.process.environment {
        #[cfg(windows)]
        environment.retain(|existing, _| !existing.to_string_lossy().eq_ignore_ascii_case(key));
        environment.insert(key.into(), value.into());
    }
    let executable = executable(
        &root,
        conf.java.executable.as_deref().unwrap_or("java"),
        &environment,
    )?;
    let mut jvm = launch.jvm.extra_arguments.clone();
    jvm.extend(conf.jvm_arguments()?);
    let arguments: Vec<_> = jvm
        .into_iter()
        .chain(launch.entry.arguments())
        .chain(launch.application.extra_arguments)
        .chain(conf.server_arguments())
        .chain(conf.application.extra_arguments)
        .collect();
    let parsed = command::JavaCommand::parse(&arguments)?;
    ensure!(
        parsed.entry == launch.entry,
        "assembled Java entry differs from declared launch entry"
    );
    // Environment options belong to Java. Only the launcher's @file switch matters
    // when mcpm itself needs to choose a transport for an oversized command.
    let estimated_length = arguments.iter().fold(0usize, |total, argument| {
        total.saturating_add(argument.len().saturating_mul(2).saturating_add(3))
    });
    let argument_file = estimated_length > if cfg!(windows) { 24_000 } else { 96_000 }
        || arguments.iter().any(|argument| argument.len() > 64_000);
    if argument_file {
        if let Some(value) = environment_value(&environment, "JDK_JAVA_OPTIONS") {
            let value = value
                .to_str()
                .context("cannot inspect JDK_JAVA_OPTIONS while preparing a Java argument file")?;
            let values = command::environment_options(value)?;
            ensure!(
                !values.iter().any(|value| value == "--disable-@files"),
                "JDK_JAVA_OPTIONS disables Java argument files; this command is too long to execute directly"
            );
        }
        ensure!(
            capabilities::argument_files(&executable, &directory, &environment)?,
            "long commands require Java 9+ argument-file support"
        );
    }
    Ok(LaunchPlan {
        executable,
        directory,
        environment,
        arguments,
        argument_file,
    })
}

fn executable(
    root: &Path,
    value: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<PathBuf> {
    if Path::new(value).is_absolute() || value.contains('/') || value.contains('\\') {
        let path = root.join(value);
        ensure!(
            is_executable(&path),
            "configured Java executable does not exist or is not executable: {path:?}"
        );
        return Ok(std::fs::canonicalize(path)?);
    }
    let path =
        environment_value(environment, "PATH").context("Java command lookup requires PATH")?;
    for directory in std::env::split_paths(path) {
        let directory = if directory.is_absolute() {
            directory
        } else {
            root.join(directory)
        };
        let candidate = directory.join(value);
        if is_executable(&candidate) {
            return Ok(std::fs::canonicalize(candidate)?);
        }
        #[cfg(windows)]
        if Path::new(value).extension().is_none() {
            let candidate = directory.join(format!("{value}.exe"));
            if is_executable(&candidate) {
                return Ok(std::fs::canonicalize(candidate)?);
            }
        }
    }
    anyhow::bail!("Java executable {value:?} was not found in the effective PATH")
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    #[cfg(windows)]
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return false;
    }
    true
}

fn environment_value<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    key: &str,
) -> Option<&'a OsStr> {
    #[cfg(windows)]
    return environment
        .iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_os_str());
    #[cfg(not(windows))]
    environment.get(OsStr::new(key)).map(OsString::as_os_str)
}
