//! Creates the single validated local Java execution plan from server-root configuration.

use crate::download::Downloader;
use crate::local::{
    java::{
        capabilities, command,
        executable::{self, JavaExecutableNotFound},
    },
    mc::server::{
        MinecraftServer, MinecraftServerError,
        authlib_injector::{self, JarPolicy},
        config::{self, Platform},
    },
};
use anyhow::{Context, Result, ensure};
use mcpm_exec::error::TippedError;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::{Event, Stage};

/// Immutable execution input; only backend preparation can construct it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct LaunchPlan {
    #[serde(with = "native_path")]
    pub(super) executable: PathBuf,
    #[serde(with = "native_path")]
    pub(super) directory: PathBuf,
    #[serde(with = "native_environment")]
    pub(super) environment: BTreeMap<OsString, OsString>,
    pub(super) command: command::JavaCommand,
    pub(super) argument_file: bool,
    pub(super) revision: config::ConfigurationRevision,
}

impl std::fmt::Debug for LaunchPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchPlan")
            .field("executable", &self.executable)
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl LaunchPlan {
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn command(&self) -> &command::JavaCommand {
        &self.command
    }
    pub fn arguments(&self) -> Vec<String> {
        self.command.arguments()
    }
    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
    }
    pub fn revision(&self) -> &config::ConfigurationRevision {
        &self.revision
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
    let (mut launch, conf, revision) = {
        let _lock = config::ConfigurationLock::acquire(&root, false)?;
        MinecraftServer::inspect(&root).map_err(MinecraftServerError::into_diagnostic)?;
        let (launch, conf) = config::read(&root, overrides)?;
        let revision = config::read_revision_locked(&root)?;
        (launch, conf, revision)
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
    let environment = executable::merge_environment(&launch.process.environment);
    let executable = executable::resolve(
        &root,
        conf.java.executable.as_deref().unwrap_or("java"),
        &environment,
    )
    .map_err(attach_executable_tip)?;
    let jvm = super::description::base_jvm(&mut launch, &conf)?;
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
    }
    let command = super::description::finish_command(&root, launch, conf, jvm)?;
    let arguments = command.arguments();
    // Environment options belong to Java. Only the launcher's @file switch matters
    // when mcpm itself needs to choose a transport for an oversized command.
    let estimated_length = arguments.iter().fold(0usize, |total, argument| {
        total.saturating_add(argument.len().saturating_mul(2).saturating_add(3))
    });
    let argument_file = estimated_length > if cfg!(windows) { 24_000 } else { 96_000 }
        || arguments.iter().any(|argument| argument.len() > 64_000);
    if argument_file {
        progress(Event::Stage(Stage::JavaCapabilities))?;
        if let Some(value) = executable::environment_value(&environment, "JDK_JAVA_OPTIONS") {
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
            capabilities::argument_files(&executable, &directory, &environment)
                .map_err(attach_executable_tip)?,
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
        command,
        argument_file,
        revision,
    })
}

fn validate_agents(arguments: &[String], directory: &Path, configured: &Path) -> Result<()> {
    let configured = agent_location(configured);
    for (name, _) in command::jvm_options(arguments) {
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

pub(super) fn attach_executable_tip(error: anyhow::Error) -> anyhow::Error {
    if let Some(not_found) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<JavaExecutableNotFound>())
    {
        let tipped = TippedError::executable_not_found(&not_found.executable);
        return TippedError::new(error, tipped.local().cloned(), tipped.remote().cloned()).into();
    }
    error
}

// Native values cross the transport only to return to their originating Host.
mod native_path {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        path.as_os_str().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        OsString::deserialize(deserializer).map(PathBuf::from)
    }
}

mod native_environment {
    use super::*;
    use serde::{
        Deserializer, Serializer,
        de::{SeqAccess, Visitor},
    };

    pub fn serialize<S: Serializer>(
        environment: &BTreeMap<OsString, OsString>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(environment.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<OsString, OsString>, D::Error> {
        struct Environment;
        impl<'de> Visitor<'de> for Environment {
            type Value = BTreeMap<OsString, OsString>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a sequence of native environment key/value pairs")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut environment = BTreeMap::new();
                while let Some((key, value)) = sequence.next_element()? {
                    environment.insert(key, value);
                }
                Ok(environment)
            }
        }
        deserializer.deserialize_seq(Environment)
    }
}
