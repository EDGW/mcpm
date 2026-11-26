//! Staged in-place mod loader installation, verification, and atomic publication.
//!
//! Orchestrates pure local mechanics for converting a v4 Vanilla Minecraft server into
//! Forge, NeoForge, or Fabric without performing network I/O or holding server locks across
//! installer execution:
//! 1. [`preflight`] validates Vanilla eligibility and snapshots inputs with zero disk writes.
//! 2. [`StagingDir`] isolates installer execution under `.mcpm/loader-install/<id>/`.
//! 3. [`stage_and_verify`] prepares adapter inputs, runs the installer JAR via [`runner`],
//!    expands/merges `launch.toml`, and verifies identity through [`crate::local::modloader::BUILTIN_LOADERS`].
//! 4. [`publish_locked`] re-verifies the snapshot under a caller-held write [`ServerLock`],
//!    writes allowlisted files after full pre-write safety checks, and commits `.mcpm/server.json`
//!    and `launch.toml` atomically while preserving `conf.toml`.

use std::{
    process::ExitStatus,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use thiserror::Error;

use crate::download::loader::LoaderFamily;

mod adapters;
mod preflight;
mod publish;
mod runner;
mod verify;

pub use super::config::PendingPublication;
pub use preflight::{PreflightSnapshot, preflight};
pub use publish::{PublishOutcome, publish_locked};
pub use runner::{
    InstallCancelled, InstallerExecution, MirrorAgentSpec, StagingDir, run_installer,
};
pub use verify::VerifiedInstall;

/// Parameters for executing a staged loader installer and verifying its output.
#[derive(Debug, Clone)]
pub struct InstallRequest<'a> {
    pub family: LoaderFamily,
    pub loader_version: &'a str,
    pub force: bool,
    pub agent: Option<&'a MirrorAgentSpec>,
    pub round: usize,
    pub cancel: Option<&'a AtomicBool>,
}

/// Output of staging, executing the installer JAR, and verifying the resulting launch state.
#[derive(Debug)]
pub struct StagedInstall {
    pub execution: InstallerExecution,
    pub verified: VerifiedInstall,
}

/// Runs adapter preparation, invokes the staged installer JAR in `staging`, and merges plus
/// verifies the resulting launch configuration and loader identity.
///
/// The caller must have already placed the downloaded installer JAR at
/// `staging.installer_jar_path()`.
pub fn stage_and_verify(
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    request: &InstallRequest<'_>,
) -> Result<StagedInstall> {
    stage_and_verify_with_hook(snapshot, staging, request, || {})
}

/// Like [`stage_and_verify`], invoking `on_verifying` once the installer exits successfully
/// and before collecting and verifying staged artifacts.
pub fn stage_and_verify_with_hook(
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    request: &InstallRequest<'_>,
    on_verifying: impl FnOnce(),
) -> Result<StagedInstall> {
    let prepared = adapters::prepare(
        request.family,
        request.loader_version,
        snapshot,
        staging,
        request.force,
    )?;

    let execution = runner::run_installer(
        &snapshot.root,
        &snapshot.launch,
        &snapshot.conf,
        staging,
        &prepared.installer_args,
        request.agent,
        request.round,
        request.cancel,
    )?;

    if !execution.status.success() {
        return Err(InstallerFailed {
            family: request.family,
            status: execution.status,
            stderr_tail: format_log_tail("stderr", &execution.stderr_tail),
            stdout_tail: format_log_tail("stdout", &execution.stdout_tail),
        }
        .into());
    }

    if request
        .cancel
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
    {
        return Err(InstallCancelled.into());
    }

    on_verifying();

    let artifacts = adapters::collect(prepared, snapshot, staging)?;
    let verified = verify::merge_and_verify(
        request.family,
        request.loader_version,
        snapshot,
        staging,
        artifacts,
    )?;

    Ok(StagedInstall {
        execution,
        verified,
    })
}

/// Returned when the loader installer subprocess exits with a non-zero status.
#[derive(Debug, Error)]
#[error("{family} installer exited with {status}{stderr_tail}{stdout_tail}")]
pub struct InstallerFailed {
    pub family: LoaderFamily,
    pub status: ExitStatus,
    stderr_tail: String,
    stdout_tail: String,
}

fn format_log_tail(stream: &str, tail: &str) -> String {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("\n--- installer {stream} tail ---\n{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
    };

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::local::{
        java::command::JavaEntry,
        mc::server::{
            MinecraftServer, MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader,
            ServerModLoader,
            config::{self, ConfAction, LaunchAction, Platform, Publication, ServerLock},
        },
    };

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write_vanilla_jar(path: &Path, mc_version: &str) {
        let version_json = format!(r#"{{"id":"{mc_version}","name":"{mc_version}"}}"#);
        write_zip(
            path,
            &[
                (
                    "META-INF/MANIFEST.MF",
                    b"Manifest-Version: 1.0\r\nMain-Class: net.minecraft.bundler.Main\r\n\r\n",
                ),
                ("version.json", version_json.as_bytes()),
            ],
        );
    }

    fn write_fabric_launch_jar(path: &Path, loader_version: &str, mc_version: Option<&str>) {
        let mut class_path = format!(
            "libraries/net/fabricmc/fabric-loader/{loader_version}/fabric-loader-{loader_version}.jar"
        );
        if let Some(mc) = mc_version {
            class_path.push_str(&format!(
                " libraries/net/fabricmc/intermediary/{mc}/intermediary-{mc}.jar"
            ));
        }
        let manifest = format!(
            "Manifest-Version: 1.0\r\nMain-Class: net.fabricmc.loader.impl.launch.server.FabricServerLauncher\r\nClass-Path: {class_path}\r\n\r\n"
        );
        write_zip(path, &[("META-INF/MANIFEST.MF", manifest.as_bytes())]);
    }

    fn init_vanilla_server(
        root: &Path,
        mc_version: &str,
        working_directory: Option<&str>,
        jar_ref: &str,
        vanilla_jar_abs: &Path,
        extra_jvm: Vec<String>,
        extra_app: Vec<String>,
    ) -> Vec<u8> {
        write_vanilla_jar(vanilla_jar_abs, mc_version);
        if let Some(wd) = working_directory {
            fs::create_dir_all(root.join(wd)).unwrap();
        }
        let launch = config::LaunchConfig {
            version: 1,
            platform: None,
            entry: JavaEntry::Jar {
                path: jar_ref.to_owned(),
            },
            jvm: config::Arguments {
                extra_arguments: extra_jvm,
            },
            application: config::Arguments {
                extra_arguments: extra_app,
            },
            process: config::Process {
                working_directory: working_directory.map(str::to_owned),
                environment: Default::default(),
            },
        };
        launch.validate().unwrap();
        let conf_text = format!(
            "# preserve me\n{}",
            config::vanilla_server().unwrap().template().unwrap()
        );
        let metadata = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: mc_version.to_owned(),
            loader: ServerLoader::Vanilla,
        };
        let lock = ServerLock::acquire_write(root).unwrap();
        config::commit(
            root,
            &lock,
            Publication {
                identity: &metadata,
                launch: LaunchAction::Write(&launch),
                conf: ConfAction::Write(&conf_text),
                replace_existing: true,
                recover_pending: false,
            },
        )
        .unwrap();
        conf_text.as_bytes().to_vec()
    }

    #[test]
    fn preflight_rejects_invalid_states_without_creating_lock_file() {
        // 1. Modded server rejected without creating .mcpm/server.lock.
        let modded_dir = tempfile::tempdir().unwrap();
        let vanilla = modded_dir.path().join("server.jar");
        init_vanilla_server(
            modded_dir.path(),
            "1.21.1",
            None,
            "server.jar",
            &vanilla,
            vec![],
            vec![],
        );
        {
            let lock = ServerLock::acquire_write(modded_dir.path()).unwrap();
            let meta = MinecraftServerMetadata {
                version: SERVER_METADATA_VERSION,
                minecraft_version: "1.21.1".to_owned(),
                loader: ServerLoader::Modded(ServerModLoader {
                    loader_type: "fabric".to_owned(),
                    version: "0.16.10".to_owned(),
                }),
            };
            config::commit(
                modded_dir.path(),
                &lock,
                Publication {
                    identity: &meta,
                    launch: LaunchAction::Keep,
                    conf: ConfAction::Keep,
                    replace_existing: true,
                    recover_pending: false,
                },
            )
            .unwrap();
        }
        fs::remove_file(modded_dir.path().join(".mcpm/server.lock")).unwrap();
        assert!(preflight(modded_dir.path()).is_err());
        assert!(
            !modded_dir.path().join(".mcpm/server.lock").exists(),
            "read-only preflight must not create server.lock"
        );

        // 2. Non-jar entry, cross-platform launch, escaping workingDirectory, mismatched JAR,
        //    and pending journal returning typed PendingPublication.
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("server.jar");
        init_vanilla_server(
            dir.path(),
            "1.21.1",
            None,
            "server.jar",
            &jar,
            vec![],
            vec![],
        );

        // Mismatched vanilla JAR version.
        write_vanilla_jar(&jar, "1.20.4");
        assert!(preflight(dir.path()).is_err());
        write_vanilla_jar(&jar, "1.21.1");

        // Cross-platform launch.toml.
        let other_platform = if Platform::current() == Platform::Windows {
            Platform::Unix
        } else {
            Platform::Windows
        };
        {
            let lock = ServerLock::acquire_write(dir.path()).unwrap();
            let mut snap = config::read_snapshot_locked(
                dir.path(),
                &lock,
                &config::ConfigurationOverrides::default(),
            )
            .unwrap();
            snap.launch.platform = Some(other_platform);
            config::commit(
                dir.path(),
                &lock,
                Publication {
                    identity: &snap.metadata,
                    launch: LaunchAction::Write(&snap.launch),
                    conf: ConfAction::Keep,
                    replace_existing: true,
                    recover_pending: false,
                },
            )
            .unwrap();
        }
        assert!(preflight(dir.path()).is_err());

        // Non-jar entry.
        {
            let lock = ServerLock::acquire_write(dir.path()).unwrap();
            let mut snap = config::read_snapshot_locked(
                dir.path(),
                &lock,
                &config::ConfigurationOverrides::default(),
            )
            .unwrap();
            snap.launch.platform = None;
            snap.launch.entry = JavaEntry::Class {
                main_class: "net.minecraft.server.Main".to_owned(),
            };
            config::commit(
                dir.path(),
                &lock,
                Publication {
                    identity: &snap.metadata,
                    launch: LaunchAction::Write(&snap.launch),
                    conf: ConfAction::Keep,
                    replace_existing: true,
                    recover_pending: false,
                },
            )
            .unwrap();
        }
        assert!(preflight(dir.path()).is_err());

        // Escaping workingDirectory.
        {
            let lock = ServerLock::acquire_write(dir.path()).unwrap();
            let mut snap = config::read_snapshot_locked(
                dir.path(),
                &lock,
                &config::ConfigurationOverrides::default(),
            )
            .unwrap();
            snap.launch.entry = JavaEntry::Jar {
                path: "server.jar".to_owned(),
            };
            snap.launch.process.working_directory = Some("..".to_owned());
            config::commit(
                dir.path(),
                &lock,
                Publication {
                    identity: &snap.metadata,
                    launch: LaunchAction::Write(&snap.launch),
                    conf: ConfAction::Keep,
                    replace_existing: true,
                    recover_pending: false,
                },
            )
            .unwrap();
        }
        assert!(preflight(dir.path()).is_err());

        // Pending journal yields typed PendingPublication.
        fs::write(dir.path().join(".mcpm/launch-publication.json"), b"{}").unwrap();
        let err = preflight(dir.path()).unwrap_err();
        assert!(
            err.downcast_ref::<PendingPublication>().is_some(),
            "expected typed PendingPublication, got: {err:#}"
        );
    }

    #[test]
    fn fabric_legacy_separated_wd_preserves_comments_enforces_conflict_and_publishes() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("jars/vanilla.jar");
        let original_conf = init_vanilla_server(
            dir.path(),
            "1.16.5",
            Some("instance"),
            "../jars/vanilla.jar",
            &vanilla,
            vec!["-Duser.old=1".to_owned()],
            vec!["--world".to_owned(), "custom".to_owned()],
        );
        // Existing properties with comment, extra key, and conflicting serverJar.
        let props_path = dir.path().join("instance/fabric-server-launcher.properties");
        fs::write(
            &props_path,
            "# keep comment\nserverJar=other.jar\ncustomKey=keepMe\n",
        )
        .unwrap();

        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();

        // Simulate Fabric installer output in stage/instance/ plus non-published junk.
        write_fabric_launch_jar(
            &staging.stage_cwd().join("fabric-server-launch.jar"),
            "0.11.7",
            None,
        );
        staging
            .write_into_cwd(Path::new("libraries/sample.jar"), b"lib-bytes")
            .unwrap();
        staging
            .write_into_cwd(Path::new("run.sh"), b"#!/bin/sh\n")
            .unwrap();
        staging
            .write_into_cwd(Path::new("installer.jar.log"), b"log")
            .unwrap();

        // Without --force, conflicting serverJar is rejected.
        let prepared_no_force =
            adapters::prepare(LoaderFamily::Fabric, "0.11.7", &snapshot, &staging, false).unwrap();
        assert!(adapters::collect(prepared_no_force, &snapshot, &staging).is_err());

        // With --force, serverJar is updated while preserving comments and extra keys.
        let prepared =
            adapters::prepare(LoaderFamily::Fabric, "0.11.7", &snapshot, &staging, true).unwrap();
        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        let verified =
            verify::merge_and_verify(LoaderFamily::Fabric, "0.11.7", &snapshot, &staging, artifacts)
                .unwrap();

        assert_eq!(verified.launch.platform, None);
        assert_eq!(
            verified.launch.process.working_directory.as_deref(),
            Some("instance")
        );
        assert_eq!(
            verified.launch.jvm.extra_arguments,
            vec!["-Duser.old=1".to_owned()]
        );

        let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
        let outcome =
            publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true).unwrap();
        drop(lock);
        staging.cleanup().unwrap();

        assert!(dir.path().join("instance/fabric-server-launch.jar").is_file());
        assert!(dir.path().join("instance/libraries/sample.jar").is_file());
        assert!(!dir.path().join("instance/run.sh").exists());
        assert!(!dir.path().join("instance/installer.jar.log").exists());
        assert!(!outcome.published_files.iter().any(|p| p.ends_with("vanilla.jar")));

        let updated_props = fs::read_to_string(&props_path).unwrap();
        assert!(updated_props.contains("# keep comment"));
        assert!(updated_props.contains("customKey=keepMe"));
        assert!(updated_props.contains("serverJar=../jars/vanilla.jar"));

        assert_eq!(fs::read(dir.path().join("conf.toml")).unwrap(), original_conf);
        let loaded = MinecraftServer::inspect(dir.path()).unwrap();
        assert_eq!(
            loaded.metadata().loader,
            ServerLoader::Modded(ServerModLoader {
                loader_type: "fabric".to_owned(),
                version: "0.11.7".to_owned(),
            })
        );
    }

    #[test]
    fn fabric_modern_rejects_shadowing_and_verifies_external_vanilla_jar() {
        let external = tempfile::tempdir().unwrap();
        let external_vanilla = external.path().join("shared-vanilla.jar");
        let external_str = external_vanilla.to_str().unwrap().to_owned();

        // 1. External vanilla JAR verifies cleanly while keeping original ref in launch.toml.
        let dir = tempfile::tempdir().unwrap();
        init_vanilla_server(
            dir.path(),
            "1.21.1",
            None,
            &external_str,
            &external_vanilla,
            vec![],
            vec![],
        );
        let snapshot = preflight(dir.path()).unwrap();
        assert!(snapshot.vanilla_jar_rel.is_none());

        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();
        write_fabric_launch_jar(
            &staging.stage_cwd().join("fabric-server-launch.jar"),
            "0.16.10",
            None,
        );
        let prepared =
            adapters::prepare(LoaderFamily::Fabric, "0.16.10", &snapshot, &staging, false).unwrap();
        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        assert!(artifacts.verification_external_vanilla_jar.is_some());
        let verified = verify::merge_and_verify(
            LoaderFamily::Fabric,
            "0.16.10",
            &snapshot,
            &staging,
            artifacts,
        )
        .unwrap();
        assert_eq!(
            verified.launch.jvm.extra_arguments,
            vec![format!("-Dfabric.gameJarPath={external_str}")]
        );

        // 2. Shadowing -Dfabric.gameJarPath.server in launch.toml is rejected.
        let shadow_dir = tempfile::tempdir().unwrap();
        let local_vanilla = shadow_dir.path().join("server.jar");
        init_vanilla_server(
            shadow_dir.path(),
            "1.21.1",
            None,
            "server.jar",
            &local_vanilla,
            vec!["-Dfabric.gameJarPath.server=other.jar".to_owned()],
            vec![],
        );
        let shadow_snap = preflight(shadow_dir.path()).unwrap();
        let shadow_staging =
            StagingDir::create(&shadow_snap.root, &shadow_snap.working_directory_rel).unwrap();
        write_fabric_launch_jar(
            &shadow_staging.stage_cwd().join("fabric-server-launch.jar"),
            "0.16.10",
            None,
        );
        let prepared = adapters::prepare(
            LoaderFamily::Fabric,
            "0.16.10",
            &shadow_snap,
            &shadow_staging,
            false,
        )
        .unwrap();
        let artifacts = adapters::collect(prepared, &shadow_snap, &shadow_staging).unwrap();
        assert!(
            verify::merge_and_verify(
                LoaderFamily::Fabric,
                "0.16.10",
                &shadow_snap,
                &shadow_staging,
                artifacts,
            )
            .is_err()
        );
    }

    #[test]
    fn fml_modern_expands_argfile_sets_platform_and_enforces_publish_safety() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("server.jar");
        let original_conf = init_vanilla_server(
            dir.path(),
            "1.21.1",
            None,
            "server.jar",
            &vanilla,
            vec!["-Dold.jvm=1".to_owned()],
            vec!["--old-app".to_owned()],
        );
        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();

        let profile = r#"{
            "spec": 1,
            "minecraft": "1.21.1",
            "serverJarPath": "{LIBRARY_DIR}/net/minecraft/server/{MINECRAFT_VERSION}/server-{MINECRAFT_VERSION}-bundled.jar",
            "processors": [
                {
                    "sides": ["server"],
                    "args": [
                        "--task", "EXTRACT_FILES",
                        "--from", "data/unix_args.txt",
                        "--to", "{ROOT}/libraries/net/neoforged/neoforge/21.1.72/unix_args.txt",
                        "--from", "data/win_args.txt",
                        "--to", "{ROOT}/libraries/net/neoforged/neoforge/21.1.72/win_args.txt"
                    ]
                }
            ]
        }"#;
        write_zip(
            staging.installer_jar_path(),
            &[("install_profile.json", profile.as_bytes())],
        );

        let prepared = adapters::prepare(
            LoaderFamily::NeoForge,
            "21.1.72",
            &snapshot,
            &staging,
            false,
        )
        .unwrap();
        assert!(
            staging
                .stage_cwd()
                .join("libraries/net/minecraft/server/1.21.1/server-1.21.1-bundled.jar")
                .is_file()
        );

        let argfile_content = concat!(
            "-p libraries/net/neoforged/fancymodloader/loader/4.0.24/loader-4.0.24.jar\n",
            "cpw.mods.bootstraplauncher.BootstrapLauncher\n",
            "--launchTarget neoforgeserver\n",
            "--fml.neoForgeVersion 21.1.72\n",
            "--fml.mcVersion 1.21.1\n",
            "--fml.fmlVersion 4.0.24\n",
        );
        staging
            .write_into_cwd(
                Path::new("libraries/net/neoforged/neoforge/21.1.72/unix_args.txt"),
                argfile_content.as_bytes(),
            )
            .unwrap();
        staging
            .write_into_cwd(
                Path::new("libraries/net/neoforged/neoforge/21.1.72/win_args.txt"),
                argfile_content.as_bytes(),
            )
            .unwrap();
        staging
            .write_into_cwd(
                Path::new("libraries/net/neoforged/fancymodloader/loader/4.0.24/loader-4.0.24.jar"),
                b"loader-jar",
            )
            .unwrap();
        // Junk excluded by allowlist.
        staging
            .write_into_cwd(Path::new("run.sh"), b"#!/bin/sh\n")
            .unwrap();
        staging
            .write_into_cwd(Path::new("user_jvm_args.txt"), b"-Xmx4G\n")
            .unwrap();
        staging
            .write_into_cwd(Path::new("installer.jar.log"), b"log\n")
            .unwrap();

        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        let verified = verify::merge_and_verify(
            LoaderFamily::NeoForge,
            "21.1.72",
            &snapshot,
            &staging,
            artifacts,
        )
        .unwrap();

        assert_eq!(verified.launch.platform, Some(Platform::current()));
        assert_eq!(
            verified.launch.entry,
            JavaEntry::Class {
                main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_owned()
            }
        );
        assert_eq!(verified.launch.jvm.extra_arguments[0], "-Dold.jvm=1");
        assert_eq!(
            verified.launch.application.extra_arguments.last().map(String::as_str),
            Some("--old-app")
        );

        // Pre-write conflict must abort with ZERO files written.
        let conflict_target =
            dir.path().join("libraries/net/neoforged/fancymodloader/loader/4.0.24/loader-4.0.24.jar");
        fs::create_dir_all(conflict_target.parent().unwrap()).unwrap();
        fs::write(&conflict_target, b"different-content").unwrap();
        {
            let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
            assert!(
                publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, false)
                    .is_err()
            );
        }
        assert!(
            !dir.path()
                .join("libraries/net/minecraft/server/1.21.1/server-1.21.1-bundled.jar")
                .exists(),
            "failed conflict check must leave zero new files behind"
        );
        assert_eq!(fs::read(&conflict_target).unwrap(), b"different-content");

        // Stale snapshot (modified conf.toml) must be rejected before writing.
        fs::write(dir.path().join("conf.toml"), b"# mutated\nversion = 1\n").unwrap();
        {
            let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
            assert!(
                publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true)
                    .is_err()
            );
        }
        fs::write(dir.path().join("conf.toml"), &original_conf).unwrap();

        // With --force and fresh snapshot, publish succeeds and skips scripts + *_args.txt.
        let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
        publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true).unwrap();
        drop(lock);
        staging.cleanup().unwrap();

        assert_eq!(fs::read(&conflict_target).unwrap(), b"loader-jar");
        assert!(!dir.path().join("run.sh").exists());
        assert!(!dir.path().join("user_jvm_args.txt").exists());
        assert!(!dir.path().join("installer.jar.log").exists());
        assert!(
            !dir.path()
                .join("libraries/net/neoforged/neoforge/21.1.72/unix_args.txt")
                .exists()
        );
        assert!(
            !dir.path()
                .join("libraries/net/neoforged/neoforge/21.1.72/win_args.txt")
                .exists()
        );
        assert_eq!(fs::read(dir.path().join("conf.toml")).unwrap(), original_conf);
    }

    #[test]
    fn vanilla_jar_is_never_overwritten_even_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("fabric-server-launch.jar");
        init_vanilla_server(
            dir.path(),
            "1.21.1",
            None,
            "fabric-server-launch.jar",
            &vanilla,
            vec![],
            vec![],
        );
        let original_vanilla_bytes = fs::read(&vanilla).unwrap();
        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();

        let mut verified = VerifiedInstall {
            metadata: MinecraftServerMetadata {
                version: SERVER_METADATA_VERSION,
                minecraft_version: "1.21.1".to_owned(),
                loader: ServerLoader::Modded(ServerModLoader {
                    loader_type: "fabric".to_owned(),
                    version: "0.16.10".to_owned(),
                }),
            },
            launch: snapshot.launch.clone(),
            publish_plan: adapters::PublishPlan {
                explicit_files: vec![PathBuf::from("fabric-server-launch.jar")],
                merged_files: Default::default(),
                include_libraries: false,
            },
        };
        staging
            .write_into_cwd(Path::new("fabric-server-launch.jar"), b"mutated-loader-jar")
            .unwrap();

        let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
        assert!(
            publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true).is_err()
        );
        assert_eq!(fs::read(&vanilla).unwrap(), original_vanilla_bytes);

        // Byte-identical file targeting the vanilla JAR path is skipped without touching it.
        staging
            .write_into_cwd(Path::new("fabric-server-launch.jar"), &original_vanilla_bytes)
            .unwrap();
        verified.launch.entry = JavaEntry::Jar {
            path: "fabric-server-launch.jar".to_owned(),
        };
        let outcome =
            publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, false).unwrap();
        assert!(outcome.published_files.is_empty());
        assert_eq!(outcome.skipped_identical_files, vec![snapshot.vanilla_jar_path]);
    }

    #[test]
    fn fabric_legacy_merges_properties_without_server_jar_with_no_force_and_guards_toctou() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("custom.jar");
        init_vanilla_server(
            dir.path(),
            "1.16.5",
            None,
            "custom.jar",
            &vanilla,
            vec![],
            vec![],
        );
        let props_path = dir.path().join("fabric-server-launcher.properties");
        let initial_props = "# header comment\nextraOption=yes\n";
        fs::write(&props_path, initial_props).unwrap();

        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();
        write_fabric_launch_jar(
            &staging.stage_cwd().join("fabric-server-launch.jar"),
            "0.11.7",
            None,
        );

        let prepared =
            adapters::prepare(LoaderFamily::Fabric, "0.11.7", &snapshot, &staging, false).unwrap();
        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        let verified =
            verify::merge_and_verify(LoaderFamily::Fabric, "0.11.7", &snapshot, &staging, artifacts)
                .unwrap();

        // Concurrent mutation of fabric-server-launcher.properties must be rejected even with --force.
        fs::write(&props_path, "# modified concurrently\nextraOption=no\n").unwrap();
        {
            let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
            assert!(
                publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true)
                    .is_err()
            );
        }
        fs::write(&props_path, initial_props).unwrap();

        // Unmodified properties file merges and publishes with force = false.
        let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
        publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, false).unwrap();
        drop(lock);
        staging.cleanup().unwrap();

        let updated = fs::read_to_string(&props_path).unwrap();
        assert!(updated.contains("# header comment"));
        assert!(updated.contains("extraOption=yes"));
        assert!(updated.contains("serverJar=custom.jar"));
    }

    #[test]
    fn publish_refuses_protected_mcpm_control_paths_even_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("server.jar");
        let original_conf = init_vanilla_server(
            dir.path(),
            "1.21.1",
            None,
            "server.jar",
            &vanilla,
            vec![],
            vec![],
        );
        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();

        for protected in ["conf.toml", "launch.toml", ".mcpm/evil.jar"] {
            let rel = PathBuf::from(protected);
            staging.write_into_cwd(&rel, b"overwrite").unwrap();
            let verified = VerifiedInstall {
                metadata: MinecraftServerMetadata {
                    version: SERVER_METADATA_VERSION,
                    minecraft_version: "1.21.1".to_owned(),
                    loader: ServerLoader::Modded(ServerModLoader {
                        loader_type: "fabric".to_owned(),
                        version: "0.16.10".to_owned(),
                    }),
                },
                launch: snapshot.launch.clone(),
                publish_plan: adapters::PublishPlan {
                    explicit_files: vec![rel],
                    merged_files: Default::default(),
                    include_libraries: false,
                },
            };
            let lock = ServerLock::acquire_write(&snapshot.root).unwrap();
            assert!(
                publish_locked(&snapshot.root, &lock, &snapshot, &verified, &staging, true)
                    .is_err(),
                "expected {protected} to be rejected"
            );
        }
        assert_eq!(fs::read(dir.path().join("conf.toml")).unwrap(), original_conf);
        assert!(!dir.path().join(".mcpm/evil.jar").exists());
    }

    #[cfg(unix)]
    #[test]
    fn fabric_verifies_in_root_symlinked_vanilla_jar() {
        let dir = tempfile::tempdir().unwrap();
        let real_vanilla = dir.path().join("jars/vanilla.jar");
        write_vanilla_jar(&real_vanilla, "1.21.1");
        std::os::unix::fs::symlink(&real_vanilla, dir.path().join("server.jar")).unwrap();

        // Register server pointing at the symlinked `server.jar`.
        let launch = config::LaunchConfig {
            version: 1,
            platform: None,
            entry: JavaEntry::Jar {
                path: "server.jar".to_owned(),
            },
            jvm: config::Arguments::default(),
            application: config::Arguments::default(),
            process: config::Process::default(),
        };
        let conf_text = config::vanilla_server().unwrap().template().unwrap();
        let metadata = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: "1.21.1".to_owned(),
            loader: ServerLoader::Vanilla,
        };
        {
            let lock = ServerLock::acquire_write(dir.path()).unwrap();
            config::commit(
                dir.path(),
                &lock,
                Publication {
                    identity: &metadata,
                    launch: LaunchAction::Write(&launch),
                    conf: ConfAction::Write(&conf_text),
                    replace_existing: true,
                    recover_pending: false,
                },
            )
            .unwrap();
        }

        let snapshot = preflight(dir.path()).unwrap();
        assert_eq!(snapshot.vanilla_jar_rel, Some(PathBuf::from("jars/vanilla.jar")));
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();
        write_fabric_launch_jar(
            &staging.stage_cwd().join("fabric-server-launch.jar"),
            "0.16.10",
            None,
        );

        let prepared =
            adapters::prepare(LoaderFamily::Fabric, "0.16.10", &snapshot, &staging, false).unwrap();
        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        let verified = verify::merge_and_verify(
            LoaderFamily::Fabric,
            "0.16.10",
            &snapshot,
            &staging,
            artifacts,
        )
        .unwrap();
        assert_eq!(
            verified.launch.jvm.extra_arguments,
            vec!["-Dfabric.gameJarPath=server.jar".to_owned()]
        );
    }

    #[test]
    fn fml_152_splits_manifest_classpath_and_excludes_preplaced_versioned_jar() {
        let dir = tempfile::tempdir().unwrap();
        let vanilla = dir.path().join("server.jar");
        init_vanilla_server(
            dir.path(),
            "1.5.2",
            None,
            "server.jar",
            &vanilla,
            vec![],
            vec![],
        );
        let snapshot = preflight(dir.path()).unwrap();
        let staging = StagingDir::create(&snapshot.root, &snapshot.working_directory_rel).unwrap();

        let profile = r#"{
            "install": {
                "minecraft": "1.5.2",
                "filePath": "minecraftforge-universal-1.5.2-7.8.1.738.jar"
            }
        }"#;
        write_zip(
            staging.installer_jar_path(),
            &[("install_profile.json", profile.as_bytes())],
        );

        let prepared =
            adapters::prepare(LoaderFamily::Forge, "7.8.1.738", &snapshot, &staging, false)
                .unwrap();
        // Installer pre-placement uses versioned name.
        assert!(staging.stage_cwd().join("minecraft_server.1.5.2.jar").is_file());

        // Simulated 1.5.2 universal JAR whose Manifest Class-Path references plain minecraft_server.jar.
        write_zip(
            &staging
                .stage_cwd()
                .join("minecraftforge-universal-1.5.2-7.8.1.738.jar"),
            &[(
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nMain-Class: net.minecraft.server.MinecraftServer\r\nClass-Path: minecraft_server.jar\r\n\r\n",
            )],
        );

        let artifacts = adapters::collect(prepared, &snapshot, &staging).unwrap();
        assert_eq!(
            artifacts.publish_plan.explicit_files,
            vec![
                PathBuf::from("minecraftforge-universal-1.5.2-7.8.1.738.jar"),
                PathBuf::from("minecraft_server.jar"),
            ]
        );
        assert!(staging.stage_cwd().join("minecraft_server.jar").is_file());
    }
}
