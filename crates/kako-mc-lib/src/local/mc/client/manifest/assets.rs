//! Client asset metadata and path resolution.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// The version manifest's `assetIndex` download declaration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexInfo {
    pub id: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub total_size: Option<u64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Contents of `assets/indexes/<id>.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    pub objects: BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub virtual_assets: bool,
    #[serde(default)]
    pub map_to_resources: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
