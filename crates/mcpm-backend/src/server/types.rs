//! Server snapshots and initialization inputs shared by server operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::data::server::config::{ConfigurationDraft, ConfigurationFiles, Published};
use crate::data::server::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerCandidate, ServerModLoader,
};
use crate::workspace::git;

/// Resolved metadata for presentation; paths retain their originating platform's spelling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerInfo {
    pub name: String,
    pub path: String,
    pub minecraft_version: String,
    pub loader: Option<LoaderInfo>,
    pub start: Option<StartCommand>,
    /// Configuration presence is only requested by a local caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ConfigurationFiles>,
}

impl ServerInfo {
    pub fn from_server(server: &crate::data::server::MinecraftServer) -> Self {
        let metadata = server.metadata();
        Self {
            name: server.name().into(),
            path: server.path().display().to_string(),
            minecraft_version: metadata.minecraft_version.clone(),
            loader: server.loader().map(|info| LoaderInfo {
                id: info.loader.id().as_str().into(),
                name: info.loader.name().into(),
                version: info.version,
            }),
            start: metadata.start.as_ref().map(|start| StartCommand {
                directory: server.path().display().to_string(),
                executable: start.executable.clone(),
                arguments: start.arguments.clone(),
                windows: start.windows,
            }),
            configuration: None,
        }
    }
}

/// Declared loader identity and version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Legacy launch snapshot retained for presentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartCommand {
    pub directory: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}

/// Partial fields restrict recognized candidates rather than inventing missing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recognize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    pub start_script: Option<PathBuf>,
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
}

impl Recognize {
    /// Validates version/loader filters before any file access.
    pub fn validate(&self) -> anyhow::Result<()> {
        for version in [&self.minecraft_version, &self.loader_version]
            .into_iter()
            .flatten()
        {
            anyhow::ensure!(!version.trim().is_empty(), "version options must not be empty");
        }
        if let Some(loader) = &self.loader {
            anyhow::ensure!(
                loader == "vanilla"
                    || kako_mc_lib::local::modloader::BUILTIN_LOADERS.contains_key(loader.as_str()),
                "unsupported server loader {loader:?}"
            );
        }
        anyhow::ensure!(
            !(self.loader.as_deref() == Some("vanilla") && self.loader_version.is_some()),
            "vanilla does not have a loader version"
        );
        Ok(())
    }

    /// Complete manual arguments bypass script recognition entirely.
    pub fn manual_metadata(&self) -> Option<MinecraftServerMetadata> {
        let minecraft_version = self.minecraft_version.as_ref()?;
        let loader = match self.loader.as_deref()? {
            "vanilla" => None,
            loader_type => Some(ServerModLoader {
                loader_type: loader_type.to_owned(),
                version: self.loader_version.as_ref()?.clone(),
            }),
        };
        Some(MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: minecraft_version.clone(),
            loader,
            start: None,
        })
    }
}

/// Complete metadata is sufficient to initialize; no prior recognition is required.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initialize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(serialize_with = "wire_metadata")]
    pub metadata: MinecraftServerMetadata,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub no_git: bool,
    #[serde(default)]
    pub git: bool,
    /// Local configuration import is deliberately excluded from the Remote protocol.
    #[serde(skip)]
    pub local: Option<LocalImport>,
}

/// Configuration draft applied when the server lives in the calling process.
#[derive(Debug)]
pub struct LocalImport {
    pub configuration: Option<ConfigurationDraft>,
    pub replace_conf: bool,
}

/// Registration result and optional Git outcome.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initialized {
    pub server: ServerInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<git::Outcome>,
    #[serde(skip)]
    pub publication: Option<Published>,
}

/// Local candidates carry complete drafts; the wire schema carries its snapshot only.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    #[serde(serialize_with = "wire_metadata")]
    pub metadata: MinecraftServerMetadata,
    pub line: usize,
    pub main_class: String,
    #[serde(skip)]
    pub configuration: Option<ConfigurationDraft>,
}

impl From<ServerCandidate> for Candidate {
    fn from(candidate: ServerCandidate) -> Self {
        Self {
            metadata: candidate.metadata,
            line: candidate.launch.line,
            main_class: candidate.launch.main_class().to_owned(),
            configuration: Some(candidate.configuration),
        }
    }
}

/// Progress phase reported while recognizing or registering a server.
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

/// Protocol v3 already carries this schema with version 1; disk upgrades stay in `data`.
fn wire_metadata<S: serde::Serializer>(
    metadata: &MinecraftServerMetadata,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut metadata = metadata.clone();
    metadata.version = 1;
    metadata.serialize(serializer)
}

/// Compatibility facts a source command derives from a registered server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModsContext {
    pub directory: PathBuf,
    pub loader: String,
    pub minecraft_version: String,
    pub cache: crate::scope::HostCache,
}
