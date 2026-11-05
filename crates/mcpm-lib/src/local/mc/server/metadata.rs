//! Local server metadata schemas, including read compatibility with format version 1.

use super::MinecraftServerError;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::Path;

/// Format emitted for newly persisted local server metadata.
pub const SERVER_METADATA_VERSION: u32 = 3;

/// Contents of a server root's `.mcpm/server.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftServerMetadata {
    /// Versions 1 and 2 remain readable. Version 3 stores registration identity only.
    #[serde(deserialize_with = "format_version")]
    pub version: u32,
    /// Minecraft version, rather than the loader version.
    #[serde(deserialize_with = "nonempty_string")]
    pub minecraft_version: String,
    /// Required in JSON; use `null` for servers without a mod loader.
    #[serde(deserialize_with = "Option::deserialize")]
    pub loader: Option<ServerModLoader>,
    /// Legacy snapshot, read for compatibility; new registration writes remove it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<ServerStartCommand>,
}

/// A flattened Java command, stored as arguments rather than executable shell text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStartCommand {
    /// Script location relative to the server root supplied when opening the server.
    pub script: std::path::PathBuf,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}

fn format_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if !matches!(version, 1 | 2 | SERVER_METADATA_VERSION) {
        return Err(serde::de::Error::custom(format!(
            "unsupported server metadata version {version}; expected 1, 2 or {SERVER_METADATA_VERSION}"
        )));
    }
    Ok(version)
}

pub(super) fn decode(
    path: &Path,
    bytes: &[u8],
) -> Result<MinecraftServerMetadata, MinecraftServerError> {
    serde_json::from_slice(bytes).map_err(|source| MinecraftServerError::ParseMetadata {
        path: path.to_owned(),
        source,
    })
}

/// Validates public inputs through the same schema, then emits identity-only registration.
pub(super) fn encode(
    root: &Path,
    metadata: &MinecraftServerMetadata,
) -> Result<(MinecraftServerMetadata, Vec<u8>), MinecraftServerError> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|source| MinecraftServerError::SerializeMetadata { source })?;
    let mut metadata = decode(&root.join(".mcpm/server.json"), &bytes)?;
    super::validate_start_script(root, &metadata)?;
    metadata.version = SERVER_METADATA_VERSION;
    metadata.start = None;
    let mut bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|source| MinecraftServerError::SerializeMetadata { source })?;
    bytes.push(b'\n');
    Ok((metadata, bytes))
}

/// A declared loader identity and version; no implementation is instantiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerModLoader {
    #[serde(rename = "type", deserialize_with = "nonempty_string")]
    pub loader_type: String,
    #[serde(deserialize_with = "nonempty_string")]
    pub version: String,
}

fn nonempty_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom("expected a nonempty string"));
    }
    Ok(value)
}
