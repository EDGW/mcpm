//! Initialization inputs and recognition candidates, independent of CLI parsing and transport.

use std::path::PathBuf;

use mcpm_backend::local::mc::server::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader, identify::ServerCandidate,
};
use serde::{Deserialize, Serialize};

/// Complete metadata is sufficient to initialize; no prior recognition or session state is required.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Initialize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(serialize_with = "wire_metadata")]
    pub metadata: MinecraftServerMetadata,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub no_git: bool,
}

/// Partial fields restrict recognized candidates rather than inventing missing metadata.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recognize {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    pub start_script: Option<PathBuf>,
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
}

impl Recognize {
    /// Complete manual arguments bypass script recognition entirely.
    pub(super) fn manual_metadata(&self) -> Option<MinecraftServerMetadata> {
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

/// Metadata carries the sole launch snapshot; provenance adds only selection context.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Candidate {
    #[serde(serialize_with = "wire_metadata")]
    pub metadata: MinecraftServerMetadata,
    pub line: usize,
    pub main_class: String,
}
impl From<ServerCandidate> for Candidate {
    fn from(candidate: ServerCandidate) -> Self {
        Self {
            metadata: candidate.metadata,
            line: candidate.launch.line,
            main_class: candidate.launch.main_class,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Recognizing,
    Validating,
    Writing,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

/// Protocol v3 already carries this schema with version 1. Keep it readable by
/// existing clients; disk format upgrades are owned by the persistence API.
fn wire_metadata<S: serde::Serializer>(
    metadata: &MinecraftServerMetadata,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut metadata = metadata.clone();
    metadata.version = 1;
    metadata.serialize(serializer)
}
