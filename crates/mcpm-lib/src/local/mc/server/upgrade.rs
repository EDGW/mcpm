//! Explicit registration upgrades with static entry evidence and optimistic concurrency checks.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{
    MinecraftServer, MinecraftServerError, MinecraftServerMetadata, SERVER_METADATA_VERSION,
    ServerLoader,
    config::{self, JavaEntry, Platform},
    identify, metadata,
};
use crate::local::java::jar;

/// Reads an upgrade proposal without changing registration or holding a lease across user input.
pub fn prepare(root: &Path) -> Result<UpgradePreview> {
    let root = fs::canonicalize(root).context("failed to resolve server directory")?;
    let _lock = config::ServerLock::acquire_read(&root)?;
    let (_, preview) = inspect(&root)?;
    Ok(preview)
}

/// Applies only a current proposal; the caller cannot supply replacement registration data.
pub fn apply(
    root: &Path,
    expected_revision: &str,
    decision: UpgradeDecision,
) -> Result<MinecraftServer> {
    let root = fs::canonicalize(root).context("failed to resolve server directory")?;
    let _lock = config::ServerLock::acquire_write(&root)?;
    config::ServerLock::require_clean(&root)?;
    let (mut metadata, preview) = inspect(&root)?;
    ensure!(
        preview.revision == expected_revision,
        UpgradeApplyError::StalePreview
    );
    if preview.from_version == SERVER_METADATA_VERSION {
        return MinecraftServer::inspect(root).map_err(MinecraftServerError::into_diagnostic);
    }
    if decision == UpgradeDecision::UseVanilla {
        ensure!(
            preview.vanilla.is_some() && matches!(metadata.loader, ServerLoader::Unrecognized),
            UpgradeApplyError::MissingVanillaEvidence
        );
        metadata.loader = ServerLoader::Vanilla;
    }
    let (metadata, bytes) = metadata::encode(&root, &metadata)?;
    // Validate the prospective snapshot without creating a mods directory or rewriting tuning.
    let server = MinecraftServer::registration_snapshot(root.clone(), metadata)?;
    metadata::publish(&root, &bytes, true)?;
    Ok(server)
}

/// Decision errors are separate from configuration, archive and filesystem failures.
#[derive(Debug, thiserror::Error)]
pub enum UpgradeApplyError {
    #[error("server registration or launch evidence changed; run info --upgrade again")]
    StalePreview,
    #[error("the current launch entry does not provide Vanilla evidence")]
    MissingVanillaEvidence,
}

/// A proposal belongs to the exact source documents used to derive its entry evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradePreview {
    pub from_version: u32,
    pub to_version: u32,
    pub revision: String,
    pub vanilla: Option<VanillaCandidate>,
    pub reason: Option<String>,
}

/// Positive main-class evidence, not a claim that the distribution is unmodified or launchable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VanillaCandidate {
    pub main_class: String,
    pub jar: Option<PathBuf>,
}

/// Cancel is intentionally absent: cancelling never submits a mutation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeDecision {
    Preserve,
    UseVanilla,
}

fn inspect(root: &Path) -> Result<(MinecraftServerMetadata, UpgradePreview)> {
    let path = root.join(".mcpm/server.json");
    let bytes = fs::read(&path).context("failed to read server registration for upgrade")?;
    let decoded = metadata::decode(&path, &bytes)?;
    let mut revision = blake3::Hasher::new();
    fingerprint(&mut revision, root.as_os_str().as_encoded_bytes());
    fingerprint(&mut revision, &bytes);
    let (vanilla, reason) = if decoded.source_version == SERVER_METADATA_VERSION {
        (
            None,
            Some("Server registration is already at the current version.".into()),
        )
    } else if matches!(decoded.metadata.loader, ServerLoader::Unrecognized) {
        inspect_launch(root, &mut revision)?
    } else {
        (
            None,
            Some("The declared loader identity will be preserved.".into()),
        )
    };
    Ok((
        decoded.metadata,
        UpgradePreview {
            from_version: decoded.source_version,
            to_version: SERVER_METADATA_VERSION,
            revision: revision.finalize().to_hex().to_string(),
            vanilla,
            reason,
        },
    ))
}

fn inspect_launch(
    root: &Path,
    revision: &mut blake3::Hasher,
) -> Result<(Option<VanillaCandidate>, Option<String>)> {
    let Some(snapshot) = config::read_launch(root)? else {
        fingerprint(revision, b"missing launch.toml");
        return Ok((
            None,
            Some("launch.toml is missing; keeping Unrecognized.".into()),
        ));
    };
    fingerprint(revision, snapshot.revision.as_bytes());
    let launch = snapshot.configuration;
    let windows = launch.platform.unwrap_or_else(Platform::current) == Platform::Windows;
    let resolve = |value: &str| {
        if windows {
            PathBuf::from(value.replace('\\', "/"))
        } else {
            PathBuf::from(value)
        }
    };
    let working = root.join(resolve(
        launch.process.working_directory.as_deref().unwrap_or("."),
    ));
    let (main_class, source_jar) = match &launch.entry {
        JavaEntry::Class { main_class } => (Some(main_class.clone()), None),
        JavaEntry::Module { main_class, .. } => (main_class.clone(), None),
        JavaEntry::Jar { path } => {
            let path = working.join(resolve(path));
            let (manifest, content) = jar::manifest_with_content(&path)?;
            fingerprint(revision, path.as_os_str().as_encoded_bytes());
            fingerprint(revision, &content);
            let main_class = manifest
                .get("main-class")
                .with_context(|| format!("launch JAR {path:?} has no Main-Class"))?
                .clone();
            (Some(main_class), Some(path))
        }
    };
    let Some(main_class) = main_class.filter(|main_class| !main_class.trim().is_empty()) else {
        return Ok((
            None,
            Some("The configured entry has no explicit main class; keeping Unrecognized.".into()),
        ));
    };
    if identify::is_vanilla_main_class(&main_class) {
        Ok((
            Some(VanillaCandidate {
                main_class,
                jar: source_jar,
            }),
            None,
        ))
    } else {
        Ok((
            None,
            Some(format!(
                "Main class {main_class:?} is not a known Vanilla entry; keeping Unrecognized."
            )),
        ))
    }
}

fn fingerprint(revision: &mut blake3::Hasher, bytes: &[u8]) {
    revision.update(&(bytes.len() as u64).to_le_bytes());
    revision.update(bytes);
}
