//! Sole TOML codec, validation, import classification and recoverable local configuration publication.

use crate::local::java::command;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

mod types;
pub use types::*;
mod application;
pub(crate) mod tuning;
pub(crate) use tuning::{managed, validate_arguments};
mod import;
pub use import::{ConfigurationDraft, ImportedSetting, ImportedValue, draft};
mod overrides;
pub use overrides::ConfigurationOverrides;
mod publication;
pub(crate) use publication::ConfigurationLock;
pub use publication::PublicationRecoveryError;
pub use publication::{OverwritePolicy, Published, publish};

/// Local configuration file presence for informational views; no Java probe or legacy migration.
#[derive(Debug)]
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

/// Reads a consistent pair while the caller owns the backend configuration lock.
pub(crate) fn read(
    root: &Path,
    overrides: &ConfigurationOverrides,
) -> Result<(LaunchConfig, ServerConfig)> {
    let launch = decode_launch(&fs::read_to_string(root.join("launch.toml")).context(
        "failed to read server-root launch.toml; explicitly import a start script first",
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
    let mut table: toml::Value = toml::from_str(text).context("invalid conf.toml")?;
    // Reject unknown fields and invalid source types before applying runtime changes.
    deserialize_conf(table.clone())?;
    overrides.apply(&mut table)?;
    let value = deserialize_conf(table)?;
    value.validate().context("invalid conf.toml")?;
    Ok(value)
}

fn deserialize_conf(value: toml::Value) -> Result<ServerConfig> {
    value.try_into().context("invalid conf.toml")
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
            !tuning::contains_managed(&self.jvm.extra_arguments)?,
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
        tuning::validate(self)
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

pub(crate) fn validate_pair(launch: &LaunchConfig, conf: &ServerConfig) -> Result<()> {
    launch.validate()?;
    conf.validate()?;
    let mut jvm = launch.jvm.extra_arguments.clone();
    jvm.extend(conf.jvm_arguments()?);
    command::validate_jvm(&jvm)
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
