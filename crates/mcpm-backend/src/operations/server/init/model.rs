//! Complete initialization inputs, recognition candidates, and serializable outcomes.

use super::{super::info::ServerInfo, git};
use mcpm_lib::local::mc::server::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader,
    config::{ConfigurationDraft, ImportedSetting, Published},
    identify::ServerCandidate,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Initialized {
    pub server: ServerInfo,
    pub git: Option<git::Outcome>,
    pub publication: Option<Publication>,
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
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
}
impl Recognize {
    pub fn manual_metadata(&self) -> anyhow::Result<Option<MinecraftServerMetadata>> {
        super::implementation::validate_options(self)?;
        let Some(minecraft_version) = self.minecraft_version.as_ref() else {
            return Ok(None);
        };
        let Some(loader_type) = self.loader.as_deref() else {
            return Ok(None);
        };
        let loader = match loader_type {
            "vanilla" => None,
            loader_type => {
                let Some(version) = self.loader_version.as_ref() else {
                    return Ok(None);
                };
                Some(ServerModLoader {
                    loader_type: loader_type.to_owned(),
                    version: version.clone(),
                })
            }
        };
        Ok(Some(MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: minecraft_version.clone(),
            loader,
            start: None,
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub metadata: MinecraftServerMetadata,
    pub line: usize,
    pub main_class: String,
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
