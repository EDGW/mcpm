//! Launch configuration merging, platform updates, and staged identity verification.
//!
//! Expands adapter-supplied launcher argv inside `stage/<workingDirectory>/` via
//! [`crate::local::java::command::expand`], merges into the existing `LaunchConfig` without
//! modifying `conf.toml`, checks Fabric JVM property shadowing and staging-path hygiene,
//! composes the full runtime [`JavaCommand`] via [`crate::local::mc::server::launch::compose_command`],
//! and verifies the assembled [`ServerLaunch`] through [`BUILTIN_LOADERS`].

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::{
    adapters::{InstalledArtifacts, PublishPlan},
    preflight::PreflightSnapshot,
    runner::{self, StagingDir},
};
use crate::{
    download::loader::LoaderFamily,
    local::{
        java::{
            command::{self, JavaEntry},
            jar,
        },
        mc::server::{
            MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLaunch, ServerLoader,
            ServerModLoader,
            config::{self, LaunchConfig, Platform},
            launch, properties,
        },
        modloader::BUILTIN_LOADERS,
    },
};

const FABRIC_LAUNCHER_PROPERTIES: &str = "fabric-server-launcher.properties";
const SERVER_JAR_KEY: &str = "serverJar";
const STAGING_MARKER: &str = ".mcpm/loader-install";

/// Verified launch configuration, server metadata, and allowlisted artifacts ready for publication.
#[derive(Debug, Clone)]
pub struct VerifiedInstall {
    pub metadata: MinecraftServerMetadata,
    pub launch: LaunchConfig,
    pub(crate) publish_plan: PublishPlan,
}

/// Expands staged installer output, merges it into `snapshot.launch`, validates cross-file
/// constraints against `snapshot.conf`, and verifies identity via [`BUILTIN_LOADERS`].
pub(crate) fn merge_and_verify(
    family: LoaderFamily,
    expected_loader_version: &str,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    artifacts: InstalledArtifacts,
) -> Result<VerifiedInstall> {
    let effective_platform = if artifacts.expanded_platform_argfile {
        Platform::current()
    } else {
        snapshot.launch.platform.unwrap_or_else(Platform::current)
    };
    let windows = effective_platform == Platform::Windows;

    let (expanded, argument_files) =
        command::expand(artifacts.initial_argv, staging.stage_cwd(), windows)
            .context("failed to expand staged loader launch arguments")?;

    if let Some(expected_jar_ref) = artifacts.fabric_game_jar_property.as_deref() {
        check_fabric_property_shadowing(
            "launch.toml",
            &snapshot.launch.jvm.extra_arguments,
            expected_jar_ref,
        )?;
        check_fabric_property_shadowing(
            "conf.toml",
            &snapshot.conf.jvm.extra_arguments,
            expected_jar_ref,
        )?;
    }

    let mut new_launch = snapshot.launch.clone();
    new_launch.entry = expanded.entry;
    new_launch.jvm.extra_arguments.extend(expanded.jvm);
    let mut merged_application = expanded.application;
    merged_application.extend(new_launch.application.extra_arguments);
    new_launch.application.extra_arguments = merged_application;
    if artifacts.expanded_platform_argfile {
        new_launch.platform = Some(Platform::current());
    }

    ensure_no_staging_paths(&new_launch, staging.base_dir())?;
    new_launch
        .validate()
        .context("merged launch.toml failed validation")?;
    config::validate_pair(&new_launch, &snapshot.conf)
        .context("merged launch.toml conflicts with existing conf.toml")?;

    verify_staged_files_exist(
        &new_launch,
        &artifacts.publish_plan,
        staging.stage_cwd(),
        windows,
    )?;

    let mut verify_command =
        launch::compose_command(&snapshot.root, new_launch.clone(), snapshot.conf.clone())
            .context("failed to assemble full verification command")?;

    let mut properties_override = None;
    if let Some(external_vanilla) = artifacts.verification_external_vanilla_jar.as_deref() {
        let external_str = external_vanilla
            .to_str()
            .context("canonical vanilla server JAR path is not valid UTF-8")?;
        if artifacts.fabric_game_jar_property.is_some() {
            // Verification-only override appended last so ServerLaunch::property resolves directly
            // to the canonical real read-only vanilla JAR while launch.toml keeps the original ref.
            verify_command
                .jvm
                .push(format!("-Dfabric.gameJarPath={external_str}"));
        } else {
            properties_override = Some(TemporaryPropertiesOverride::apply(
                &staging.stage_cwd().join(FABRIC_LAUNCHER_PROPERTIES),
                external_str,
            )?);
        }
    }

    let path_separator = if windows { ';' } else { ':' };
    let jar_manifest = match &verify_command.entry {
        JavaEntry::Jar { path } => {
            let entry_jar = resolve_stage_path(staging.stage_cwd(), path, windows);
            jar::manifest(&entry_jar).with_context(|| {
                format!("failed to read JAR manifest of staged entry {entry_jar:?}")
            })?
        }
        _ => HashMap::new(),
    };

    let server_launch = ServerLaunch {
        executable: snapshot
            .conf
            .java
            .executable
            .clone()
            .unwrap_or_else(|| "java".to_owned()),
        script: snapshot.root.join("launch.toml"),
        line: 0,
        directory: staging.stage_cwd().to_path_buf(),
        command: verify_command,
        environment: new_launch
            .process
            .environment
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        argument_files,
        jar_manifest,
        path_separator,
    };

    let identify_result = identify_staged_server(
        &server_launch,
        family,
        expected_loader_version,
        &snapshot.minecraft_version,
    );
    if let Some(guard) = properties_override {
        guard.restore()?;
    }
    identify_result?;

    Ok(VerifiedInstall {
        metadata: MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: snapshot.minecraft_version.clone(),
            loader: ServerLoader::Modded(ServerModLoader {
                loader_type: family.as_str().to_owned(),
                version: expected_loader_version.to_owned(),
            }),
        },
        launch: new_launch,
        publish_plan: artifacts.publish_plan,
    })
}

fn check_fabric_property_shadowing(
    source: &str,
    jvm_arguments: &[String],
    expected_jar_ref: &str,
) -> Result<()> {
    for (name, value) in command::jvm_options(jvm_arguments) {
        if name == "-Dfabric.gameJarPath.server" {
            bail!(
                "{source} defines -Dfabric.gameJarPath.server, which overrides -Dfabric.gameJarPath and would shadow vanilla JAR {expected_jar_ref:?}"
            );
        }
        if name == "-Dfabric.gameJarPath" && value != Some(expected_jar_ref) {
            bail!(
                "{source} defines conflicting -Dfabric.gameJarPath={value:?}; expected {expected_jar_ref:?}"
            );
        }
    }
    Ok(())
}

fn ensure_no_staging_paths(launch: &LaunchConfig, staging_base: &Path) -> Result<()> {
    let base_norm = normalize_path_for_check(&staging_base.to_string_lossy());
    let marker = normalize_path_for_check(STAGING_MARKER);
    let check = |value: &str, field: &str| -> Result<()> {
        let normalized = normalize_path_for_check(value);
        ensure!(
            !normalized.contains(&marker)
                && (base_norm.is_empty() || !normalized.contains(&base_norm)),
            "merged launch.toml contains a staging workspace path in {field}: {value:?}"
        );
        Ok(())
    };

    match &launch.entry {
        JavaEntry::Class { main_class } => check(main_class, "entry.mainClass")?,
        JavaEntry::Jar { path } => check(path, "entry.path")?,
        JavaEntry::Module { module, main_class } => {
            check(module, "entry.module")?;
            if let Some(main_class) = main_class {
                check(main_class, "entry.mainClass")?;
            }
        }
    }
    for arg in &launch.jvm.extra_arguments {
        check(arg, "jvm.extraArguments")?;
    }
    for arg in &launch.application.extra_arguments {
        check(arg, "application.extraArguments")?;
    }
    if let Some(wd) = &launch.process.working_directory {
        check(wd, "process.workingDirectory")?;
    }
    for (key, value) in &launch.process.environment {
        check(value, &format!("process.environment.{key}"))?;
    }
    Ok(())
}

fn verify_staged_files_exist(
    launch: &LaunchConfig,
    publish_plan: &PublishPlan,
    stage_cwd: &Path,
    windows: bool,
) -> Result<()> {
    if let JavaEntry::Jar { path } = &launch.entry {
        let entry_path = resolve_stage_path(stage_cwd, path, windows);
        let meta = fs::symlink_metadata(&entry_path).with_context(|| {
            format!("staged entry JAR {entry_path:?} referenced by launch.toml does not exist")
        })?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "staged entry JAR {entry_path:?} must be a regular file"
        );
    }

    for rel in &publish_plan.explicit_files {
        runner::ensure_clean_relative(rel, "publish plan file")?;
        let path = stage_cwd.join(rel);
        let meta = fs::symlink_metadata(&path).with_context(|| {
            format!("expected staged runtime file {path:?} is missing from staging directory")
        })?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "staged runtime file {path:?} must be a regular file"
        );
    }
    Ok(())
}

fn identify_staged_server(
    server_launch: &ServerLaunch,
    expected_family: LoaderFamily,
    expected_loader_version: &str,
    expected_mc_version: &str,
) -> Result<()> {
    let mut loaders: Vec<_> = BUILTIN_LOADERS.values().collect();
    loaders.sort_by(|a, b| a.id().cmp(b.id()));

    let mut matched = Vec::new();
    for loader in loaders {
        if let Some(identity) = loader
            .instance()
            .try_identify_server(server_launch)
            .with_context(|| {
                format!(
                    "loader verification failed while inspecting staged server with {}",
                    loader.name()
                )
            })?
        {
            matched.push((loader.id().as_str().to_owned(), identity));
        }
    }

    ensure!(
        !matched.is_empty(),
        "staged server was not recognized as {} {expected_loader_version} for Minecraft {expected_mc_version}",
        expected_family.as_str()
    );
    ensure!(
        matched.len() == 1,
        "staged server matched multiple loaders ({matched:?}); expected only {}",
        expected_family.as_str()
    );

    let (detected_loader, identity) = &matched[0];
    ensure!(
        detected_loader == expected_family.as_str(),
        "staged server identified as loader {detected_loader:?}, expected {:?}",
        expected_family.as_str()
    );
    ensure!(
        identity.version == expected_loader_version,
        "staged server identified as {} version {:?}, expected {expected_loader_version:?}",
        expected_family.as_str(),
        identity.version
    );
    ensure!(
        identity.minecraft_version == expected_mc_version,
        "staged server identified for Minecraft {:?}, expected {expected_mc_version:?}",
        identity.minecraft_version
    );
    Ok(())
}

fn resolve_stage_path(stage_cwd: &Path, path: &str, windows: bool) -> PathBuf {
    let normalized = if windows && !cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    stage_cwd.join(normalized)
}

fn normalize_path_for_check(value: &str) -> String {
    let slashes = value.replace('\\', "/");
    if cfg!(windows) {
        slashes.to_ascii_lowercase()
    } else {
        slashes
    }
}

/// Temporarily overrides `serverJar` in `stage_cwd/fabric-server-launcher.properties` for
/// verification of out-of-root vanilla JARs on Fabric `< 0.12.0`, always restoring the exact
/// original staged bytes before publication.
struct TemporaryPropertiesOverride {
    path: PathBuf,
    original: Option<Vec<u8>>,
    restored: bool,
}

impl TemporaryPropertiesOverride {
    fn apply(path: &Path, external_vanilla_path: &str) -> Result<Self> {
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read staged {path:?}"));
            }
        };
        let overridden = properties::upsert_key(
            original.as_deref(),
            SERVER_JAR_KEY,
            external_vanilla_path,
        )
        .context("failed to prepare verification-only Fabric serverJar override")?;
        fs::write(path, &overridden)
            .with_context(|| format!("failed to write verification-only {path:?}"))?;
        Ok(Self {
            path: path.to_path_buf(),
            original,
            restored: false,
        })
    }

    fn restore(mut self) -> Result<()> {
        self.restored = true;
        restore_properties(&self.path, self.original.as_deref())
    }
}

impl Drop for TemporaryPropertiesOverride {
    fn drop(&mut self) {
        if !self.restored
            && let Err(error) = restore_properties(&self.path, self.original.as_deref())
        {
            tracing::warn!(
                path = %self.path.display(),
                %error,
                "failed to restore temporary Fabric verification properties on drop"
            );
        }
    }
}

fn restore_properties(path: &Path, original: Option<&[u8]>) -> Result<()> {
    match original {
        Some(bytes) => fs::write(path, bytes)
            .with_context(|| format!("failed to restore staged {path:?}")),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("failed to remove temporary {path:?}"))
            }
        },
    }
}
