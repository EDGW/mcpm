//! Complete initialization inputs, recognition candidates, and serializable outcomes.

use super::{super::info::ServerInfo, git};
use mcpm_lib::local::mc::server::{
    MinecraftServerMetadata,
    config::{
        ConfigurationDraft, ConfigurationOverrides, ImportedSetting, LaunchConfig, Published,
    },
    identify::ServerCandidate,
    properties,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initialize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    pub metadata: MinecraftServerMetadata,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub no_git: bool,
    #[serde(default)]
    pub git: bool,
    pub local: Option<LocalImport>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalImport {
    pub configuration: Option<ConfigurationDraft>,
    pub replace_conf: bool,
    #[serde(default, skip_serializing_if = "ConfigurationOverrides::is_empty")]
    pub conf_overrides: ConfigurationOverrides,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preserve_conf: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon: Option<InitRconRequest>,
}

/// RCON configuration request executed on the server host during `Init`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum InitRconRequest {
    /// Explicit CLI request (`--rcon`, `--rcon-port`, `--rcon-password`):
    /// Fails before publishing registration if the version or properties cannot support the request.
    Explicit {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    /// Confirmed in Console mode: creates or losslessly updates `server.properties`.
    InteractiveEnable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
        #[serde(default)]
        generate_password: bool,
    },
    /// User declined enabling an incomplete `enable-rcon=true` configuration: set `enable-rcon=false`.
    Disable,
}

impl std::fmt::Debug for InitRconRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Explicit { port, password } => f
                .debug_struct("Explicit")
                .field("port", port)
                .field("password", &password.as_ref().map(|_| "<redacted>"))
                .finish(),
            Self::InteractiveEnable {
                port,
                generate_password,
            } => f
                .debug_struct("InteractiveEnable")
                .field("port", port)
                .field("generate_password", generate_password)
                .finish(),
            Self::Disable => f.write_str("Disable"),
        }
    }
}

/// Outcome of applying an [`InitRconRequest`] during `Init`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InitRconOutcome {
    Enabled {
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        generated_password: Option<String>,
    },
    Disabled,
    Warning {
        message: String,
    },
}

impl std::fmt::Debug for InitRconOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enabled {
                port,
                generated_password,
            } => f
                .debug_struct("Enabled")
                .field("port", port)
                .field(
                    "generated_password",
                    &generated_password.as_ref().map(|_| "<redacted>"),
                )
                .finish(),
            Self::Disabled => f.write_str("Disabled"),
            Self::Warning { message } => {
                f.debug_struct("Warning").field("message", message).finish()
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Initialized {
    pub server: ServerInfo,
    pub git: Option<git::Outcome>,
    pub publication: Option<Publication>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon_outcome: Option<InitRconOutcome>,
}

/// Host-side read-only probe input for `server.properties` RCON status before interactive `init`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRcon {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<LaunchConfig>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_conf: bool,
    #[serde(default, skip_serializing_if = "ConfigurationOverrides::is_empty")]
    pub conf_overrides: ConfigurationOverrides,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preserve_conf: bool,
}

/// Host-side RCON readiness status returned by [`super::ProbeInitRcon`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InitRconProbe {
    AlreadyComplete,
    NeedsPrompt {
        currently_enabled: bool,
        existing_port: Option<u16>,
        has_password: bool,
        invalid_port: Option<String>,
    },
    ParseFailed {
        reason: String,
    },
}

impl From<properties::InitRconProbe> for InitRconProbe {
    fn from(value: properties::InitRconProbe) -> Self {
        match value {
            properties::InitRconProbe::AlreadyComplete => Self::AlreadyComplete,
            properties::InitRconProbe::NeedsPrompt {
                currently_enabled,
                existing_port,
                has_password,
                invalid_port,
            } => Self::NeedsPrompt {
                currently_enabled,
                existing_port,
                has_password,
                invalid_port,
            },
            properties::InitRconProbe::ParseFailed { reason } => Self::ParseFailed { reason },
        }
    }
}

/// Publication diagnostics without a second copy of the server registration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Publication {
    pub created: Vec<PathBuf>,
    pub replaced: Vec<PathBuf>,
    pub preserved: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub imported: Vec<ImportedSetting>,
    pub entry_configured: bool,
}
impl From<Published> for Publication {
    fn from(value: Published) -> Self {
        Self {
            created: value.created,
            replaced: value.replaced,
            preserved: value.preserved,
            removed: value.removed,
            imported: value.imported,
            entry_configured: value.entry_configured,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recognize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    pub start_script: Option<PathBuf>,
    #[serde(default)]
    pub ignore_unknown_vars: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub metadata: MinecraftServerMetadata,
    pub script: PathBuf,
    pub line: usize,
    pub main_class: String,
    pub configuration: Option<ConfigurationDraft>,
}
impl Candidate {
    pub(super) fn from_server(candidate: ServerCandidate, root: &Path) -> anyhow::Result<Self> {
        use anyhow::Context;
        let script = std::fs::canonicalize(&candidate.launch.script)
            .context("failed to resolve candidate script")?;
        let script = script
            .strip_prefix(root)
            .context("candidate script escapes server root")?
            .to_owned();
        Ok(Self {
            metadata: candidate.metadata,
            line: candidate.launch.line,
            main_class: candidate.launch.main_class().to_owned(),
            script,
            configuration: Some(candidate.configuration),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Recognizing,
    Validating,
    Writing,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}
