//! Creates the single validated local Java execution plan from server-root configuration.

use crate::download::Downloader;
use crate::local::{
    java::{capabilities, command},
    mc::server::{
        MinecraftServer,
        authlib_injector::{self, JarPolicy},
        config::{self, Platform},
    },
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use super::{Event, Stage};

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
pub async fn prepare(
    root: &Path,
    downloader: impl FnOnce() -> Result<Downloader>,
) -> Result<LaunchPlan> {
    prepare_with_overrides(
        root,
        &config::ConfigurationOverrides::default(),
        None,
        downloader,
    )
    .await
}

/// Builds a plan from the effective launch-only configuration without rewriting either file.
pub async fn prepare_with_overrides(
    root: &Path,
    overrides: &config::ConfigurationOverrides,
    auth_policy: Option<JarPolicy>,
    downloader: impl FnOnce() -> Result<Downloader>,
) -> Result<LaunchPlan> {
    prepare_with_progress(root, overrides, auth_policy, downloader, &|_| Ok(())).await
}

/// Observes preparation through a caller-owned callback without changing plan ownership.
pub async fn prepare_with_progress(
    root: &Path,
    overrides: &config::ConfigurationOverrides,
    auth_policy: Option<JarPolicy>,
    downloader: impl FnOnce() -> Result<Downloader>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<LaunchPlan> {
    progress(Event::Stage(Stage::Configuration))?;
    let root = std::fs::canonicalize(root).context("failed to resolve local server root")?;
    let (launch, conf) = {
        let _lock = config::ConfigurationLock::acquire(&root, false)?;
        MinecraftServer::inspect(&root)?;
        let (launch, conf) = config::read(&root, overrides)?;
        (launch, conf)
    };
    progress(Event::Stage(Stage::LocalChecks))?;
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
    let managed = conf
        .authlib_injector
        .as_ref()
        .is_some_and(|auth| auth.manual_jar().is_none());
    ensure!(
        auth_policy.is_none() || managed,
        "--auth-jar-policy requires an enabled authlib-injector configuration with a managed JAR"
    );
    if let Some(auth) = &conf.authlib_injector {
        let path = match auth.manual_jar() {
            Some(path) => {
                progress(Event::Authentication(authlib_injector::Event::Checking))?;
                let path = authlib_injector::manual(&root, path)?;
                progress(Event::Authentication(authlib_injector::Event::Ready))?;
                path
            }
            None => authlib_injector::managed_path(&root),
        };
        validate_agents(&jvm, &directory, &path)?;
        jvm.extend(auth.arguments(&path)?);
    }
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
        progress(Event::Stage(Stage::JavaCapabilities))?;
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
    // Local validation and capability probes precede any installation or network request.
    if managed {
        authlib_injector::prepare_with_progress(
            &root,
            auth_policy.unwrap_or_default(),
            downloader,
            &|event| progress(Event::Authentication(event)),
        )
        .await?;
    }
    progress(Event::Stage(Stage::Ready))?;
    Ok(LaunchPlan {
        executable,
        directory,
        environment,
        arguments,
        argument_file,
    })
}

fn validate_agents(arguments: &[String], directory: &Path, configured: &Path) -> Result<()> {
    let configured = agent_location(configured);
    let mut probe = arguments.to_vec();
    probe.push("mcpm.validation.Main".into());
    for (name, _) in command::JavaCommand::parse(&probe)?.options() {
        if let Some(path) = name.strip_prefix("-javaagent:") {
            let path = directory.join(path);
            let location = agent_location(&path);
            let same = location == configured;
            #[cfg(windows)]
            let same = same
                || location
                    .as_os_str()
                    .as_encoded_bytes()
                    .eq_ignore_ascii_case(configured.as_os_str().as_encoded_bytes());
            ensure!(
                !same && !authlib_injector::is_agent(&path),
                "authlib-injector agent is configured both as a structured table and a raw JVM argument: {path:?}"
            );
        }
    }
    Ok(())
}

/// Resolves existing prefixes while retaining a missing installation tail for duplicate checks.
fn agent_location(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            _ => {
                result.push(component.as_os_str());
                if let Ok(resolved) = std::fs::canonicalize(&result) {
                    result = resolved;
                }
            }
        }
    }
    result
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
