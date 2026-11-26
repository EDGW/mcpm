//! Fabric Installer adapter honoring the Loader 0.12.0 `serverJar` vs `-Dfabric.gameJarPath` boundary.
//!
//! Runs Fabric Installer with `-dir <stage_cwd>` (without `-downloadMinecraft`), keeps the
//! pre-install staging cwd empty, uses [`crate::local::modloader::impls::fabric::supports_game_jar_property`]
//! for the 0.12.0 boundary, and updates `fabric-server-launcher.properties` through the sole
//! [`crate::local::mc::server::properties`] codec when required by older loaders.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{InstalledArtifacts, PublishPlan};
use crate::local::{
    mc::server::{
        loader_install::{preflight::PreflightSnapshot, runner::StagingDir},
        properties,
    },
    modloader::impls::fabric,
};

const FABRIC_LAUNCH_JAR: &str = "fabric-server-launch.jar";
const FABRIC_LAUNCHER_PROPERTIES: &str = "fabric-server-launcher.properties";
const SERVER_JAR_KEY: &str = "serverJar";
const DEFAULT_FABRIC_SERVER_JAR: &str = "server.jar";

#[derive(Debug)]
pub(super) struct FabricPlan {
    supports_game_jar_property: bool,
    force: bool,
}

pub(super) fn prepare(
    loader_version: &str,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    force: bool,
) -> Result<(Vec<String>, FabricPlan)> {
    let supports_game_jar_property = fabric::supports_game_jar_property(loader_version)?;
    let cwd = staging
        .stage_cwd()
        .to_str()
        .context("staging working directory is not valid UTF-8")?;
    let installer_args = vec![
        "server".to_owned(),
        "-mcversion".to_owned(),
        snapshot.minecraft_version.clone(),
        "-loader".to_owned(),
        loader_version.to_owned(),
        "-dir".to_owned(),
        cwd.to_owned(),
    ];
    Ok((
        installer_args,
        FabricPlan {
            supports_game_jar_property,
            force,
        },
    ))
}

pub(super) fn collect(
    plan: FabricPlan,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
) -> Result<InstalledArtifacts> {
    let launch_jar = PathBuf::from(FABRIC_LAUNCH_JAR);
    ensure!(
        staging.stage_cwd().join(&launch_jar).is_file(),
        "Fabric installer did not produce {FABRIC_LAUNCH_JAR}"
    );

    let mut explicit_files = vec![launch_jar];
    let mut merged_files = BTreeMap::new();
    let (initial_argv, fabric_game_jar_property, effective_jar_ref) =
        if plan.supports_game_jar_property {
            (
                vec![
                    format!("-Dfabric.gameJarPath={}", snapshot.vanilla_jar_ref),
                    "-jar".to_owned(),
                    FABRIC_LAUNCH_JAR.to_owned(),
                ],
                Some(snapshot.vanilla_jar_ref.clone()),
                snapshot.vanilla_jar_ref.clone(),
            )
        } else {
            let legacy = prepare_legacy_properties(snapshot, staging, plan.force)?;
            if let Some(staged_props) = legacy.publish_file {
                if let Some(original_hash) = legacy.merged_original_hash {
                    merged_files.insert(staged_props.clone(), original_hash);
                }
                explicit_files.push(staged_props);
            }
            (
                vec!["-jar".to_owned(), FABRIC_LAUNCH_JAR.to_owned()],
                None,
                legacy.effective_jar_ref,
            )
        };

    // Place a read-only verification copy into `stage_root` only when Fabric's effective jar
    // reference resolves lexically inside `stage_root` without colliding with staged artifacts.
    // Otherwise verify directly against the canonical read-only host path so symlinks, non-canonical
    // `sub/../` references, out-of-root JARs, and filename collisions all work without polluting `stage/`.
    let verification_external_vanilla_jar = match lexical_stage_target(
        &snapshot.working_directory_rel,
        &effective_jar_ref,
        &explicit_files,
    ) {
        Some(rel_in_stage) => {
            staging.copy_into_stage_root(&snapshot.vanilla_jar_path, &rel_in_stage)?;
            None
        }
        None => Some(snapshot.vanilla_jar_path.clone()),
    };

    Ok(InstalledArtifacts {
        initial_argv,
        expanded_platform_argfile: false,
        publish_plan: PublishPlan {
            explicit_files,
            merged_files,
            include_libraries: true,
        },
        fabric_game_jar_property,
        verification_external_vanilla_jar,
    })
}

struct LegacyPropertiesOutcome {
    /// Staged `fabric-server-launcher.properties` path when a write/merge must be published.
    publish_file: Option<PathBuf>,
    /// Original BLAKE3 digest when an existing properties file was merged in place.
    merged_original_hash: Option<String>,
    /// The exact `serverJar` reference Fabric `< 0.12.0` will resolve at verification time.
    effective_jar_ref: String,
}

/// Prepares `fabric-server-launcher.properties` for Fabric Loader `< 0.12.0`.
fn prepare_legacy_properties(
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    force: bool,
) -> Result<LegacyPropertiesOutcome> {
    let props_rel = PathBuf::from(FABRIC_LAUNCHER_PROPERTIES);
    let existing_path = snapshot.working_directory.join(FABRIC_LAUNCHER_PROPERTIES);
    let existing_bytes = read_optional_regular_file(&existing_path)?;

    let default_matches = points_to_same_file(
        &snapshot.working_directory.join(DEFAULT_FABRIC_SERVER_JAR),
        &snapshot.vanilla_jar_path,
    );

    match existing_bytes {
        Some(bytes) => {
            let original_hash = blake3::hash(&bytes).to_hex().to_string();
            let parsed = properties::parse_bytes(&bytes)
                .with_context(|| format!("failed to parse existing {existing_path:?}"))?;
            match parsed.get(SERVER_JAR_KEY) {
                Some(configured) => {
                    let configured_matches = configured == &snapshot.vanilla_jar_ref
                        || points_to_same_file(
                            &snapshot.working_directory.join(configured),
                            &snapshot.vanilla_jar_path,
                        );
                    if configured_matches {
                        // Stage the existing file for verification only; do not rewrite on publish.
                        staging.write_into_cwd(&props_rel, &bytes)?;
                        Ok(LegacyPropertiesOutcome {
                            publish_file: None,
                            merged_original_hash: None,
                            effective_jar_ref: configured.clone(),
                        })
                    } else {
                        ensure!(
                            force,
                            "{FABRIC_LAUNCHER_PROPERTIES} already configures {SERVER_JAR_KEY}={configured:?}, which conflicts with vanilla JAR {:?}; pass --force to replace it",
                            snapshot.vanilla_jar_ref
                        );
                        let updated = properties::upsert_key(
                            Some(&bytes),
                            SERVER_JAR_KEY,
                            &snapshot.vanilla_jar_ref,
                        )?;
                        staging.write_into_cwd(&props_rel, &updated)?;
                        Ok(LegacyPropertiesOutcome {
                            publish_file: Some(props_rel),
                            merged_original_hash: Some(original_hash),
                            effective_jar_ref: snapshot.vanilla_jar_ref.clone(),
                        })
                    }
                }
                None => {
                    if default_matches {
                        staging.write_into_cwd(&props_rel, &bytes)?;
                        Ok(LegacyPropertiesOutcome {
                            publish_file: None,
                            merged_original_hash: None,
                            effective_jar_ref: DEFAULT_FABRIC_SERVER_JAR.to_owned(),
                        })
                    } else {
                        let updated = properties::upsert_key(
                            Some(&bytes),
                            SERVER_JAR_KEY,
                            &snapshot.vanilla_jar_ref,
                        )?;
                        staging.write_into_cwd(&props_rel, &updated)?;
                        Ok(LegacyPropertiesOutcome {
                            publish_file: Some(props_rel),
                            merged_original_hash: Some(original_hash),
                            effective_jar_ref: snapshot.vanilla_jar_ref.clone(),
                        })
                    }
                }
            }
        }
        None => {
            if default_matches {
                Ok(LegacyPropertiesOutcome {
                    publish_file: None,
                    merged_original_hash: None,
                    effective_jar_ref: DEFAULT_FABRIC_SERVER_JAR.to_owned(),
                })
            } else {
                let created =
                    properties::upsert_key(None, SERVER_JAR_KEY, &snapshot.vanilla_jar_ref)?;
                staging.write_into_cwd(&props_rel, &created)?;
                Ok(LegacyPropertiesOutcome {
                    publish_file: Some(props_rel),
                    merged_original_hash: None,
                    effective_jar_ref: snapshot.vanilla_jar_ref.clone(),
                })
            }
        }
    }
}

/// Resolves `raw_ref` lexically relative to `working_directory_rel` within `stage_root`.
///
/// Returns `Some(rel_in_stage_root)` only when `raw_ref` is a clean relative path without
/// non-leading `..` segments, stays inside `stage_root`, and does not collide with staged
/// explicit files, `libraries/`, or `.mcpm/`.
fn lexical_stage_target(
    working_directory_rel: &Path,
    raw_ref: &str,
    explicit_files: &[PathBuf],
) -> Option<PathBuf> {
    if raw_ref.trim().is_empty() || raw_ref.contains('\0') {
        return None;
    }
    let normalized = if cfg!(windows) {
        raw_ref.replace('\\', "/")
    } else {
        raw_ref.to_owned()
    };
    if normalized.starts_with('/') {
        return None;
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }

    let mut stack: Vec<OsString> = working_directory_rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect();
    let mut seen_normal = false;
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if seen_normal {
                    // Non-leading `..` (e.g. `sub/../server.jar`) requires `stage/sub/` to exist.
                    return None;
                }
                stack.pop()?;
            }
            part => {
                seen_normal = true;
                stack.push(OsString::from(part));
            }
        }
    }
    if stack.is_empty() {
        return None;
    }
    if stack[0].to_string_lossy().eq_ignore_ascii_case(".mcpm") {
        return None;
    }

    let rel_in_stage: PathBuf = stack.into_iter().collect();
    let cwd_prefix = working_directory_rel;
    if explicit_files
        .iter()
        .any(|file| cwd_prefix.join(file) == rel_in_stage)
    {
        return None;
    }
    if rel_in_stage.starts_with(cwd_prefix.join("libraries")) {
        return None;
    }
    Some(rel_in_stage)
}

fn read_optional_regular_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {path:?}")),
        Ok(meta) => {
            ensure!(
                meta.is_file(),
                "{path:?} exists and is not a regular file"
            );
            fs::read(path)
                .map(Some)
                .with_context(|| format!("failed to read {path:?}"))
        }
    }
}

fn points_to_same_file(candidate: &Path, canonical_target: &Path) -> bool {
    std::fs::canonicalize(candidate)
        .map(|resolved| resolved == canonical_target)
        .unwrap_or(false)
}
