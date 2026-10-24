//! Local server metadata schemas, including read compatibility with format version 1.

use serde::{Deserialize, Deserializer, Serialize};

/// Format emitted for newly persisted local server metadata.
pub const SERVER_METADATA_VERSION: u32 = 2;

/// Contents of a server root's `.mcpm/server.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftServerMetadata {
    /// Configuration format version. Versions 1 and 2 are accepted; writes use version 2.
    #[serde(deserialize_with = "format_version")]
    pub version: u32,
    /// Minecraft version, rather than the loader version.
    #[serde(deserialize_with = "nonempty_string")]
    pub minecraft_version: String,
    /// Required in JSON; use `null` for servers without a mod loader.
    #[serde(deserialize_with = "Option::deserialize")]
    pub loader: Option<ServerModLoader>,
    /// Selected static launch snapshot, absent in manually registered/older configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<ServerStartCommand>,
}

/// A flattened Java command, stored as arguments rather than executable shell text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStartCommand {
    pub script: std::path::PathBuf,
    pub directory: std::path::PathBuf,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}

fn format_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != 1 && version != SERVER_METADATA_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported server metadata version {version}; expected 1 or {SERVER_METADATA_VERSION}"
        )));
    }
    Ok(version)
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
