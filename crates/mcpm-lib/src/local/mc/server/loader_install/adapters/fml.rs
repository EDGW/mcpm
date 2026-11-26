//! Unified FML (`install_profile.json`) installer adapter for Forge and NeoForge.
//!
//! Reads `install_profile.json` exclusively from `*-installer.jar` (never scanning
//! directories or parsing `run.sh`/`run.bat`) and supports:
//! - Legacy Forge (`install` / `versionInfo`, including 1.5.2 manifest `Class-Path` split and 1.6–1.12);
//! - Transitional Forge (v1 profile without `serverJarPath`, 1.13–1.16);
//! - Modern Forge (1.17+) and all NeoForge releases (`serverJarPath` + `EXTRACT_FILES`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::{
    InstalledArtifacts, PublishPlan, expand_profile_path, maven_coord_root_filenames,
    validate_safe_relative_path,
};
use crate::{
    download::loader::LoaderFamily,
    local::{
        java::{
            command::{self, JavaEntry},
            jar,
        },
        mc::server::loader_install::{preflight::PreflightSnapshot, runner::StagingDir},
    },
};

#[derive(Debug)]
pub(super) enum FmlPlan {
    Legacy {
        mc_version: String,
        launch_jar: PathBuf,
    },
    Transitional {
        mc_version: String,
        primary_jar: String,
        fallback_jar: Option<String>,
    },
    Modern {
        argfile_rel: PathBuf,
    },
}

pub(super) fn prepare(
    family: LoaderFamily,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
) -> Result<(Vec<String>, FmlPlan)> {
    let profile_bytes = jar::read(staging.installer_jar_path(), "install_profile.json")?
        .with_context(|| {
            format!(
                "{family} installer JAR {:?} is missing install_profile.json",
                staging.installer_jar_path()
            )
        })?;
    let profile: RawProfile = serde_json::from_slice(&profile_bytes).with_context(|| {
        format!(
            "failed to parse install_profile.json in {:?}",
            staging.installer_jar_path()
        )
    })?;

    let mc = snapshot.minecraft_version.as_str();
    let plan = if let Some(install) = profile.install {
        ensure!(
            family == LoaderFamily::Forge,
            "unexpected legacy Forge install_profile.json layout for {family}"
        );
        ensure!(
            install.minecraft == mc,
            "installer targets Minecraft {:?}, but server is registered as {mc:?}",
            install.minecraft
        );
        let launch_jar = validate_safe_relative_path(&install.file_path, "install.filePath")?;
        let preplaced = PathBuf::from(format!("minecraft_server.{mc}.jar"));
        staging.copy_into_cwd(&snapshot.vanilla_jar_path, &preplaced)?;
        FmlPlan::Legacy {
            mc_version: mc.to_owned(),
            launch_jar,
        }
    } else {
        if let Some(profile_mc) = profile.minecraft.as_deref() {
            ensure!(
                profile_mc == mc,
                "installer targets Minecraft {profile_mc:?}, but server is registered as {mc:?}"
            );
        }
        if let Some(server_jar_template) = profile.server_jar_path.as_deref() {
            let server_jar_rel = expand_profile_path(server_jar_template, mc)?;
            let argfile_rel = extract_platform_argfile(&profile.processors, mc)?;
            staging.copy_into_cwd(&snapshot.vanilla_jar_path, &server_jar_rel)?;
            FmlPlan::Modern { argfile_rel }
        } else if let Some(coord) = profile.path.as_deref() {
            let (primary_jar, fallback_jar) = maven_coord_root_filenames(coord)?;
            let preplaced = PathBuf::from(format!("minecraft_server.{mc}.jar"));
            staging.copy_into_cwd(&snapshot.vanilla_jar_path, &preplaced)?;
            FmlPlan::Transitional {
                mc_version: mc.to_owned(),
                primary_jar,
                fallback_jar,
            }
        } else {
            bail!(
                "unrecognized install_profile.json layout in {:?}: missing install, serverJarPath, and path",
                staging.installer_jar_path()
            );
        }
    };

    Ok((vec!["--installServer".to_owned()], plan))
}

pub(super) fn collect(
    plan: FmlPlan,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
) -> Result<InstalledArtifacts> {
    match plan {
        FmlPlan::Legacy {
            mc_version,
            launch_jar,
        } => {
            let launch_jar_abs = staging.stage_cwd().join(&launch_jar);
            ensure!(
                launch_jar_abs.is_file(),
                "installer did not produce expected Forge launch JAR {launch_jar:?}"
            );
            let versioned_vanilla = format!("minecraft_server.{mc_version}.jar");
            let runtime_vanilla = if manifest_uses_unversioned_vanilla(&launch_jar_abs, &versioned_vanilla)? {
                let plain = PathBuf::from("minecraft_server.jar");
                staging.copy_into_cwd(&snapshot.vanilla_jar_path, &plain)?;
                plain
            } else {
                PathBuf::from(versioned_vanilla)
            };
            Ok(InstalledArtifacts {
                initial_argv: vec!["-jar".to_owned(), slash_path(&launch_jar)?],
                expanded_platform_argfile: false,
                publish_plan: PublishPlan {
                    explicit_files: vec![launch_jar, runtime_vanilla],
                    merged_files: Default::default(),
                    include_libraries: true,
                },
                fabric_game_jar_property: None,
                verification_external_vanilla_jar: None,
            })
        }
        FmlPlan::Transitional {
            mc_version,
            primary_jar,
            fallback_jar,
        } => {
            let launch_jar = if staging.stage_cwd().join(&primary_jar).is_file() {
                PathBuf::from(primary_jar)
            } else if let Some(alt) = fallback_jar
                && staging.stage_cwd().join(&alt).is_file()
            {
                PathBuf::from(alt)
            } else {
                bail!("installer did not produce expected Forge launch JAR for profile.path");
            };
            let runtime_vanilla = PathBuf::from(format!("minecraft_server.{mc_version}.jar"));
            Ok(InstalledArtifacts {
                initial_argv: vec!["-jar".to_owned(), slash_path(&launch_jar)?],
                expanded_platform_argfile: false,
                publish_plan: PublishPlan {
                    explicit_files: vec![launch_jar, runtime_vanilla],
                    merged_files: Default::default(),
                    include_libraries: true,
                },
                fabric_game_jar_property: None,
                verification_external_vanilla_jar: None,
            })
        }
        FmlPlan::Modern { argfile_rel } => {
            let argfile_abs = staging.stage_cwd().join(&argfile_rel);
            ensure!(
                argfile_abs.is_file(),
                "installer did not produce expected platform argument file {argfile_rel:?}"
            );
            let initial_argv = vec![format!("@{}", slash_path(&argfile_rel)?)];

            // Detect an optional top-level shim JAR (e.g. Forge 1.20.4+ `forge-...-shim.jar`)
            // directly from the expanded argfile entry without scanning the directory.
            let (expanded, _) =
                command::expand(initial_argv.clone(), staging.stage_cwd(), cfg!(windows))
                    .with_context(|| {
                        format!("failed to expand installer argument file {argfile_rel:?}")
                    })?;
            let mut explicit_files = Vec::new();
            if let JavaEntry::Jar { path } = &expanded.entry {
                let rel = validate_safe_relative_path(path, "installer entry JAR")?;
                if !rel.starts_with("libraries") {
                    explicit_files.push(rel);
                }
            }

            Ok(InstalledArtifacts {
                initial_argv,
                expanded_platform_argfile: true,
                publish_plan: PublishPlan {
                    explicit_files,
                    merged_files: Default::default(),
                    include_libraries: true,
                },
                fabric_game_jar_property: None,
                verification_external_vanilla_jar: None,
            })
        }
    }
}

fn manifest_uses_unversioned_vanilla(launch_jar: &Path, versioned_name: &str) -> Result<bool> {
    let manifest = jar::manifest(launch_jar)?;
    let Some(class_path) = manifest.get("class-path") else {
        return Ok(false);
    };
    let mut uses_plain = false;
    let mut uses_versioned = false;
    for token in class_path.split_whitespace() {
        let name = token.rsplit('/').next().unwrap_or(token);
        if name == "minecraft_server.jar" {
            uses_plain = true;
        } else if name == versioned_name {
            uses_versioned = true;
        }
    }
    Ok(uses_plain && !uses_versioned)
}

fn extract_platform_argfile(processors: &[Processor], mc_version: &str) -> Result<PathBuf> {
    let target_from = if cfg!(windows) {
        "data/win_args.txt"
    } else {
        "data/unix_args.txt"
    };
    let mut matched = None;
    for processor in processors {
        if !processor.runs_on_server() {
            continue;
        }
        if !processor.is_extract_files() {
            continue;
        }
        let mut pending_from: Option<&str> = None;
        let mut index = 0;
        while index < processor.args.len() {
            match processor.args[index].as_str() {
                "--from" => {
                    pending_from = processor.args.get(index + 1).map(String::as_str);
                    index += 2;
                }
                "--to" => {
                    if let (Some(from), Some(to)) =
                        (pending_from.take(), processor.args.get(index + 1))
                    {
                        let normalized_from = from.trim_start_matches('/');
                        if normalized_from == target_from {
                            let expanded = expand_profile_path(to, mc_version)?;
                            if let Some(previous) = &matched {
                                ensure!(
                                    previous == &expanded,
                                    "conflicting EXTRACT_FILES targets for {target_from}: {previous:?} vs {expanded:?}"
                                );
                            }
                            matched = Some(expanded);
                        }
                    }
                    index += 2;
                }
                _ => {
                    index += 1;
                }
            }
        }
    }
    matched.with_context(|| {
        format!(
            "install_profile.json does not declare an EXTRACT_FILES --to target for {target_from}"
        )
    })
}

fn slash_path(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .with_context(|| format!("path is not valid UTF-8: {path:?}"))?;
    Ok(text.replace('\\', "/"))
}

#[derive(Deserialize)]
struct RawProfile {
    install: Option<LegacyInstall>,
    minecraft: Option<String>,
    #[serde(rename = "serverJarPath")]
    server_jar_path: Option<String>,
    path: Option<String>,
    #[serde(default)]
    processors: Vec<Processor>,
}

#[derive(Deserialize)]
struct LegacyInstall {
    minecraft: String,
    #[serde(rename = "filePath")]
    file_path: String,
}

#[derive(Deserialize)]
struct Processor {
    #[serde(default)]
    sides: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
}

impl Processor {
    fn runs_on_server(&self) -> bool {
        self.sides.is_empty() || self.sides.iter().any(|side| side == "server")
    }

    fn is_extract_files(&self) -> bool {
        self.args.iter().any(|arg| arg == "EXTRACT_FILES")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_platform_argfile_selects_host_target_from_extract_files_processor() {
        let processors = vec![
            Processor {
                sides: vec!["client".into()],
                args: vec![
                    "--task".into(),
                    "EXTRACT_FILES".into(),
                    "--from".into(),
                    "data/unix_args.txt".into(),
                    "--to".into(),
                    "{ROOT}/libraries/client_only/unix_args.txt".into(),
                ],
            },
            Processor {
                sides: vec!["server".into()],
                args: vec![
                    "--task".into(),
                    "EXTRACT_FILES".into(),
                    "--archive".into(),
                    "{INSTALLER}".into(),
                    "--from".into(),
                    "/data/unix_args.txt".into(),
                    "--to".into(),
                    "{ROOT}/libraries/net/neoforged/neoforge/21.1.72/unix_args.txt".into(),
                    "--from".into(),
                    "data/win_args.txt".into(),
                    "--to".into(),
                    "{ROOT}/libraries/net/neoforged/neoforge/21.1.72/win_args.txt".into(),
                ],
            },
        ];

        let target = extract_platform_argfile(&processors, "1.21.1").unwrap();
        let expected = if cfg!(windows) {
            "libraries/net/neoforged/neoforge/21.1.72/win_args.txt"
        } else {
            "libraries/net/neoforged/neoforge/21.1.72/unix_args.txt"
        };
        assert_eq!(target, PathBuf::from(expected));
    }

    #[test]
    fn extract_platform_argfile_rejects_missing_or_conflicting_targets() {
        assert!(extract_platform_argfile(&[], "1.21.1").is_err());
        let conflicting = vec![Processor {
            sides: vec!["server".into()],
            args: vec![
                "EXTRACT_FILES".into(),
                "--from".into(),
                if cfg!(windows) {
                    "data/win_args.txt"
                } else {
                    "data/unix_args.txt"
                }
                .into(),
                "--to".into(),
                "{ROOT}/libraries/a/args.txt".into(),
                "--from".into(),
                if cfg!(windows) {
                    "data/win_args.txt"
                } else {
                    "data/unix_args.txt"
                }
                .into(),
                "--to".into(),
                "{ROOT}/libraries/b/args.txt".into(),
            ],
        }];
        assert!(extract_platform_argfile(&conflicting, "1.21.1").is_err());
    }
}
