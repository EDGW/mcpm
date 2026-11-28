//! Sole TOML codec, validation, import classification and recoverable local configuration publication.

use crate::local::{
    java::command,
    mc::server::{
        MinecraftServerError, MinecraftServerMetadata, SERVER_METADATA_VERSION, metadata,
    },
};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

mod types;
pub use types::*;
mod application;
mod authlib_injector;
pub use authlib_injector::{AuthlibInjector, FeatureOption};
pub(crate) mod tuning;
pub(crate) use tuning::{managed, validate_arguments};
mod import;
pub use import::{
    ConfigurationDraft, ImportedSetting, ImportedValue, OverwritePolicy, commit_import, draft,
};
mod generate;
mod template;
pub use generate::vanilla_server;
mod overrides;
pub use overrides::{ConfigurationOverrides, ConfigurationShortcut};
mod publication;
pub use publication::{
    ConfAction, LaunchAction, PendingPublication, Publication, PublicationRecoveryError, Published,
    ServerLock, commit, recover_if_pending,
};

/// Local configuration file presence for informational views; no Java probe or legacy migration.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ConfigurationFiles {
    pub launch: bool,
    pub conf: bool,
}

pub fn configuration_files(root: &Path) -> Result<ConfigurationFiles> {
    Ok(ConfigurationFiles {
        launch: root
            .join("launch.toml")
            .try_exists()
            .context("failed to inspect launch.toml")?,
        conf: root
            .join("conf.toml")
            .try_exists()
            .context("failed to inspect conf.toml")?,
    })
}

/// Single-read validated server configuration snapshot with exact content hashes.
pub struct ServerConfigSnapshot {
    pub metadata: MinecraftServerMetadata,
    pub launch: LaunchConfig,
    pub conf: ServerConfig,
    pub revision: ConfigurationRevision,
    pub conf_hash: String,
}

/// Reads `.mcpm/server.json`, `launch.toml`, and `conf.toml` once each while the caller holds
/// [`ServerLock`], validating current metadata, both TOML files, and cross-file constraints,
/// while computing BLAKE3 hashes from the exact bytes read.
pub fn read_snapshot_locked(
    root: &Path,
    lock: &ServerLock,
    overrides: &ConfigurationOverrides,
) -> Result<ServerConfigSnapshot> {
    lock.ensure_held_for(root, "reading server configuration snapshot")?;
    ServerLock::require_clean(root)?;
    let manifest = root.join(".mcpm/server.json");
    let metadata_bytes = fs::read(&manifest).map_err(|source| {
        MinecraftServerError::ReadMetadata {
            path: manifest.clone(),
            source,
        }
        .into_diagnostic()
    })?;
    let metadata_hash = blake3::hash(&metadata_bytes).to_hex().to_string();
    let decoded = metadata::decode(&manifest, &metadata_bytes)
        .map_err(MinecraftServerError::into_diagnostic)?;
    if decoded.source_version != SERVER_METADATA_VERSION {
        return Err(MinecraftServerError::UpgradeRequired {
            path: manifest,
            version: decoded.source_version,
        }
        .into_diagnostic());
    }
    let launch_bytes = fs::read(root.join("launch.toml")).context(
        "failed to read server-root launch.toml; use init --entry or import a start script first",
    )?;
    let launch_hash = blake3::hash(&launch_bytes).to_hex().to_string();
    let launch_text = std::str::from_utf8(&launch_bytes).context("launch.toml is not UTF-8")?;
    let launch = decode_launch(launch_text)?;
    let conf_bytes =
        fs::read(root.join("conf.toml")).context("failed to read server-root conf.toml")?;
    let conf_hash = blake3::hash(&conf_bytes).to_hex().to_string();
    let conf_text = std::str::from_utf8(&conf_bytes).context("conf.toml is not UTF-8")?;
    let conf = decode_conf_with_overrides(conf_text, overrides)?;
    validate_pair(&launch, &conf)?;
    Ok(ServerConfigSnapshot {
        metadata: decoded.metadata,
        launch,
        conf,
        revision: ConfigurationRevision {
            metadata_hash,
            launch_hash,
        },
        conf_hash,
    })
}

/// Content revision of server registration and launch structure, independent of conf.toml.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConfigurationRevision {
    pub metadata_hash: String,
    pub launch_hash: String,
}

/// Reads the current registration and launch content revision under a shared server lock.
pub fn read_revision(root: &Path) -> Result<ConfigurationRevision> {
    let lock = ServerLock::acquire_read(root)?;
    read_revision_locked(root, &lock)
}

/// Computes effective `conf.toml` application extra arguments for a server directory
/// (combining any retained `conf.toml` with pending `overrides`), without mutating disk.
pub fn effective_conf_application_args(
    root: &Path,
    replace_conf: bool,
    preserve_conf: bool,
    overrides: &ConfigurationOverrides,
) -> Result<Vec<String>> {
    let conf_path = root.join("conf.toml");
    let existing = if replace_conf {
        None
    } else {
        match fs::read_to_string(&conf_path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {conf_path:?}"));
            }
        }
    };
    let effective = match existing {
        Some(text) if preserve_conf => decode_conf(&text)?,
        Some(text) => decode_conf_with_overrides(&text, overrides)?,
        None => {
            if overrides.is_empty() {
                ServerConfig::default()
            } else {
                decode_conf_with_overrides(&template::default_template(), overrides)?
            }
        }
    };
    Ok(effective.application.extra_arguments)
}

/// Reads `launch.toml` and `conf.toml` when present, returning `(launch, conf_app_args)`
/// for read-only inspection (such as resolving `server.properties` in `info`).
pub fn read_properties_context(root: &Path) -> Result<(Option<LaunchConfig>, Vec<String>)> {
    let launch_path = root.join("launch.toml");
    let launch = match fs::read_to_string(&launch_path) {
        Ok(text) => Some(decode_launch(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {launch_path:?}"));
        }
    };
    let conf_args =
        effective_conf_application_args(root, false, true, &ConfigurationOverrides::default())?;
    Ok((launch, conf_args))
}

/// Reads raw registration and launch hashes while the caller holds [`ServerLock`].
pub fn read_revision_locked(root: &Path, lock: &ServerLock) -> Result<ConfigurationRevision> {
    lock.ensure_held_for(root, "reading server configuration revision")?;
    ServerLock::require_clean(root)?;
    let metadata = fs::read(root.join(".mcpm/server.json"))
        .context("failed to read server registration .mcpm/server.json")?;
    let launch = fs::read(root.join("launch.toml")).context(
        "failed to read server-root launch.toml; use init --entry or import a start script first",
    )?;
    Ok(ConfigurationRevision {
        metadata_hash: blake3::hash(&metadata).to_hex().to_string(),
        launch_hash: blake3::hash(&launch).to_hex().to_string(),
    })
}

/// Typed launch data and the exact document revision; no user tuning is loaded.
pub(crate) struct LaunchSnapshot {
    pub(crate) configuration: LaunchConfig,
    pub(crate) revision: blake3::Hash,
}

/// Reads only launch.toml while the caller holds the configuration lock.
pub(crate) fn read_launch(root: &Path) -> Result<Option<LaunchSnapshot>> {
    let path = root.join("launch.toml");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(&path) {
                Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(other) => return Err(other).context("failed to inspect launch.toml"),
                Ok(_) => return Err(error).context("failed to read existing launch.toml"),
            }
        }
        Err(error) => return Err(error).context("failed to read launch.toml"),
    };
    let text = std::str::from_utf8(&bytes).context("launch.toml is not UTF-8")?;
    Ok(Some(LaunchSnapshot {
        configuration: decode_launch(text)?,
        revision: blake3::hash(&bytes),
    }))
}

/// Reads a consistent pair while the caller owns the backend configuration lock.
pub(crate) fn read(
    root: &Path,
    overrides: &ConfigurationOverrides,
) -> Result<(LaunchConfig, ServerConfig)> {
    let launch = decode_launch(&fs::read_to_string(root.join("launch.toml")).context(
        "failed to read server-root launch.toml; use init --entry or import a start script first",
    )?)?;
    let conf = decode_conf_with_overrides(
        &fs::read_to_string(root.join("conf.toml"))
            .context("failed to read server-root conf.toml")?,
        overrides,
    )?;
    validate_pair(&launch, &conf)?;
    Ok((launch, conf))
}

/// Decodes and validates fixed launch structure; no filesystem or Java execution.
pub fn decode_launch(text: &str) -> Result<LaunchConfig> {
    let value: LaunchConfig = toml::from_str(text).context("invalid launch.toml")?;
    value.validate().context("invalid launch.toml")?;
    Ok(value)
}

/// Decodes and validates user settings; comments never become settings.
pub fn decode_conf(text: &str) -> Result<ServerConfig> {
    decode_conf_with_overrides(text, &ConfigurationOverrides::default())
}

/// Applies launch-only changes before validating the effective configuration.
pub fn decode_conf_with_overrides(
    text: &str,
    overrides: &ConfigurationOverrides,
) -> Result<ServerConfig> {
    let mut value = toml::from_str::<ServerConfig>(text).context("invalid conf.toml")?;
    overrides.apply(&mut value)?;
    value.validate().context("invalid conf.toml")?;
    Ok(value)
}

impl LaunchConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported launch.toml version {}",
            self.version
        );
        self.entry.validate()?;
        if let Some(path) = &self.process.working_directory {
            nonempty(path, "process.workingDirectory")?;
            ensure!(
                !Path::new(path).is_absolute(),
                "workingDirectory must be relative to the server root"
            );
        }
        for (name, value) in &self.process.environment {
            nonempty(name, "environment key")?;
            ensure!(
                !name.contains('='),
                "environment key contains '=': {name:?}"
            );
            ensure!(!value.contains('\0'), "NUL in environment value for {name}");
        }
        command::validate_jvm(&self.jvm.extra_arguments)?;
        ensure!(
            self.jvm
                .extra_arguments
                .iter()
                .any(|argument| argument == "--disable-@files")
                || !self
                    .entry
                    .arguments()
                    .iter()
                    .any(|argument| argument.starts_with('@')),
            "live Java @argument files are forbidden in the configured entry"
        );
        ensure!(
            !tuning::contains_managed(&self.jvm.extra_arguments),
            "launch.toml contains memory/GC settings; put them exclusively in conf.toml"
        );
        validate_arguments(&self.application.extra_arguments)?;
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<String> {
        self.validate()?;
        toml::to_string_pretty(self).context("failed to encode launch.toml")
    }
}

impl ServerConfig {
    pub fn validate(&self) -> Result<()> {
        tuning::validate(self)?;
        if let Some(auth) = &self.authlib_injector {
            auth.validate()?;
            auth.validate_properties(&self.jvm.extra_arguments)?;
        }
        self.restart_on.validate()?;
        Ok(())
    }
    pub(crate) fn jvm_arguments(&self) -> Result<Vec<String>> {
        tuning::generate(self)
    }

    pub(crate) fn server_arguments(&self) -> Vec<String> {
        let mut result = Vec::new();
        if self.server.nogui == Some(true) {
            result.push("nogui".into());
        }
        if let Some(port) = self.server.port {
            result.extend(["--port".into(), port.to_string()]);
        }
        if let Some(world) = &self.server.world {
            result.extend(["--world".into(), world.clone()]);
        }
        result
    }
}

/// Checks cross-file constraints; callers must validate `launch` and `conf` individually first.
pub(crate) fn validate_pair(launch: &LaunchConfig, conf: &ServerConfig) -> Result<()> {
    if let Some(auth) = &conf.authlib_injector {
        auth.validate_properties(&launch.jvm.extra_arguments)?;
    }
    Ok(())
}

pub(crate) fn nonempty(value: &str, field: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && !value.contains('\0'),
        "{field} must be nonempty and contain no NUL"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
