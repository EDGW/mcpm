//! Read-only preflight validation and input snapshotting for in-place loader installation.
//!
//! Preflight performs zero disk writes: it acquires a non-creating shared [`ServerLock`],
//! reads `.mcpm/server.json`, `launch.toml`, and `conf.toml` once via [`config::read_snapshot_locked`],
//! validates v4 Vanilla eligibility, entry shape, path boundaries, and vanilla JAR identity,
//! and records BLAKE3 hashes for optimistic re-verification at commit time.

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::super::{
    ServerLoader,
    config::{
        self, ConfigurationOverrides, ConfigurationRevision, LaunchConfig, Platform, ServerConfig,
        ServerLock,
    },
    identify,
};
use crate::local::java::command::JavaEntry;

/// Validated read-only input state captured before staging or downloading anything.
#[derive(Debug, Clone)]
pub struct PreflightSnapshot {
    /// Canonicalized server root directory.
    pub root: PathBuf,
    /// Declared and JAR-verified Minecraft version.
    pub minecraft_version: String,
    /// Existing validated `launch.toml`.
    pub launch: LaunchConfig,
    /// Existing validated `conf.toml` (never modified by loader installation).
    pub conf: ServerConfig,
    /// Content revision of `.mcpm/server.json` and `launch.toml`.
    pub revision: ConfigurationRevision,
    /// Raw BLAKE3 hex digest of `conf.toml`.
    pub conf_hash: String,
    /// Canonicalized `process.workingDirectory`, guaranteed to lie inside `root`.
    pub working_directory: PathBuf,
    /// `working_directory` relative to `root` (empty when equal to `root`).
    pub working_directory_rel: PathBuf,
    /// Original `JavaEntry::Jar { path }` string preserved verbatim from `launch.toml`.
    pub vanilla_jar_ref: String,
    /// Canonicalized real read-only vanilla server JAR path.
    pub vanilla_jar_path: PathBuf,
    /// Path of `vanilla_jar_path` relative to `root` when contained inside `root`.
    pub vanilla_jar_rel: Option<PathBuf>,
    /// BLAKE3 hex digest of `vanilla_jar_path`.
    pub vanilla_jar_hash: String,
}

impl PreflightSnapshot {
    /// Whether `working_directory` is the server root itself.
    pub fn working_directory_is_root(&self) -> bool {
        self.working_directory_rel.as_os_str().is_empty()
    }
}

/// Validates that `root` is an eligible v4 Vanilla server with a `-jar` entry and
/// captures a consistent read-only input snapshot without writing to disk.
///
/// If an interrupted v1 publication journal (`.mcpm/launch-publication.json`) is present,
/// this returns [`config::PendingPublication`] so the caller (`mcpm-backend`) can recover
/// it under an exclusive lock and abort cleanly.
pub fn preflight(root: &Path) -> Result<PreflightSnapshot> {
    ensure!(
        root.is_dir(),
        "server root is not an existing directory: {root:?}"
    );
    let root =
        std::fs::canonicalize(root).with_context(|| format!("failed to resolve server root {root:?}"))?;

    let snapshot = {
        let lock = ServerLock::acquire_read(&root)?;
        config::read_snapshot_locked(&root, &lock, &ConfigurationOverrides::default())?
    };

    ensure!(
        snapshot.metadata.loader == ServerLoader::Vanilla,
        "add-loader only supports servers registered as Vanilla; current loader is {:?}",
        snapshot.metadata.loader
    );

    ensure!(
        snapshot
            .launch
            .platform
            .is_none_or(|platform| platform == Platform::current()),
        "launch.toml targets a different platform ({:?}); cannot convert on {:?}",
        snapshot.launch.platform,
        Platform::current()
    );

    let vanilla_jar_ref = match &snapshot.launch.entry {
        JavaEntry::Jar { path } => path.clone(),
        other => bail!(
            "add-loader requires a -jar entry in launch.toml (found {other:?}); run `mcpm init --entry` first"
        ),
    };

    let (working_directory, working_directory_rel) =
        resolve_working_directory(&root, snapshot.launch.process.working_directory.as_deref())?;

    let vanilla_jar_path = resolve_vanilla_jar(&working_directory, &vanilla_jar_ref)?;

    let detected_mc = identify::identify_jar(&vanilla_jar_path)
        .with_context(|| format!("failed to identify vanilla server JAR {vanilla_jar_path:?}"))?
        .with_context(|| {
            format!(
                "configured entry JAR {vanilla_jar_path:?} is not a recognized vanilla Minecraft server JAR"
            )
        })?;
    ensure!(
        detected_mc == snapshot.metadata.minecraft_version,
        "vanilla server JAR {vanilla_jar_path:?} is Minecraft {detected_mc:?}, which does not match registered version {:?}",
        snapshot.metadata.minecraft_version
    );

    let vanilla_jar_rel = vanilla_jar_path
        .strip_prefix(&root)
        .ok()
        .map(Path::to_path_buf);
    let vanilla_jar_hash = hash_file(&vanilla_jar_path)?;

    Ok(PreflightSnapshot {
        root,
        minecraft_version: snapshot.metadata.minecraft_version,
        launch: snapshot.launch,
        conf: snapshot.conf,
        revision: snapshot.revision,
        conf_hash: snapshot.conf_hash,
        working_directory,
        working_directory_rel,
        vanilla_jar_ref,
        vanilla_jar_path,
        vanilla_jar_rel,
        vanilla_jar_hash,
    })
}

/// Re-verifies under a held [`ServerLock`] that `.mcpm/server.json`, `launch.toml`,
/// `conf.toml`, and the source vanilla JAR are byte-identical to preflight.
pub(crate) fn verify_snapshot_fresh(
    root: &Path,
    lock: &ServerLock,
    snapshot: &PreflightSnapshot,
) -> Result<()> {
    let current_revision = config::read_revision_locked(root, lock)?;
    ensure!(
        current_revision == snapshot.revision,
        "server registration or launch.toml changed during loader installation; retry the command"
    );

    let current_conf_hash = hash_file(&root.join("conf.toml"))
        .context("failed to re-hash conf.toml before loader publication")?;
    ensure!(
        current_conf_hash == snapshot.conf_hash,
        "conf.toml changed during loader installation; retry the command"
    );

    let current_vanilla_hash = hash_file(&snapshot.vanilla_jar_path)
        .with_context(|| format!("failed to re-hash vanilla JAR {:?}", snapshot.vanilla_jar_path))?;
    ensure!(
        current_vanilla_hash == snapshot.vanilla_jar_hash,
        "vanilla server JAR {:?} changed during loader installation; retry the command",
        snapshot.vanilla_jar_path
    );

    Ok(())
}

/// Streams a regular file into BLAKE3 without buffering the entire file in memory.
pub(crate) fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("failed to open {path:?} for hashing"))?;
    let mut hasher = blake3::Hasher::new();
    hasher
        .update_reader(&mut file)
        .with_context(|| format!("failed to hash {path:?}"))?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn resolve_working_directory(
    canonical_root: &Path,
    configured: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let raw = configured.unwrap_or(".");
    ensure!(
        !Path::new(raw).is_absolute(),
        "process.workingDirectory must be relative to the server root: {raw:?}"
    );
    let candidate = canonical_root.join(raw);
    let canonical = std::fs::canonicalize(&candidate).with_context(|| {
        format!("configured process.workingDirectory does not exist: {candidate:?}")
    })?;
    ensure!(
        canonical.is_dir(),
        "configured process.workingDirectory is not a directory: {canonical:?}"
    );
    let relative = canonical
        .strip_prefix(canonical_root)
        .with_context(|| {
            format!(
                "process.workingDirectory {canonical:?} escapes server root {canonical_root:?}"
            )
        })?
        .to_path_buf();
    Ok((canonical, relative))
}

fn resolve_vanilla_jar(working_directory: &Path, jar_ref: &str) -> Result<PathBuf> {
    let candidate = working_directory.join(jar_ref);
    let canonical = std::fs::canonicalize(&candidate)
        .with_context(|| format!("configured vanilla server JAR does not exist: {candidate:?}"))?;
    ensure!(
        canonical.is_file(),
        "configured vanilla server JAR is not a regular file: {canonical:?}"
    );
    Ok(canonical)
}
