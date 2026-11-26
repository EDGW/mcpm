//! Read-only legacy DTOs: versions 1 and 2 deliberately use the old V3 schema.

use serde::Deserialize;

use super::nonempty_string;

/// Migration data only; no server behavior or persistence implementation.
#[derive(Deserialize)]
pub(super) struct MinecraftServerMetadataV3 {
    #[serde(rename = "version")]
    _version: u32,
    #[serde(deserialize_with = "nonempty_string")]
    pub(super) minecraft_version: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(super) loader: Option<ServerModLoaderV3>,
}

#[derive(Deserialize)]
pub(super) struct ServerModLoaderV3 {
    #[serde(rename = "type", deserialize_with = "nonempty_string")]
    pub(super) loader_type: String,
    #[serde(deserialize_with = "nonempty_string")]
    pub(super) version: String,
}
