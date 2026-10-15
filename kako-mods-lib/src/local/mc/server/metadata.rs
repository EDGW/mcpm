use serde::{Deserialize, Deserializer, Serialize};

/// Contents of a server root's `.mcpm/server.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftServerMetadata {
    /// Configuration format version. Currently only version 1 is supported.
    #[serde(deserialize_with = "format_version")]
    pub version: u32,
    /// Minecraft version, rather than the loader version.
    #[serde(deserialize_with = "nonempty_string")]
    pub minecraft_version: String,
    /// Required in JSON; use `null` for servers without a mod loader.
    #[serde(deserialize_with = "Option::deserialize")]
    pub loader: Option<ServerModLoader>,
}

fn format_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != 1 {
        return Err(serde::de::Error::custom(format!(
            "unsupported server metadata version {version}; expected 1"
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
