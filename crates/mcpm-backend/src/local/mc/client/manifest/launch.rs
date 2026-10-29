use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// Java requirement declared by a version or loader patch.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRequirement {
    pub component: Option<String>,
    pub major_version: u32,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientLogging {
    pub argument: String,
    pub file: LoggingFile,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl super::MinecraftVersionManifest {
    /// Normalize each document before external inheritance. HMCL root profiles
    /// contain a derived root snapshot plus authoritative patches; combining both
    /// would duplicate game/JVM arguments. Inspection keeps its separate snapshot.
    pub(crate) fn into_launch_manifest(mut self) -> Self {
        let mut patches = std::mem::take(&mut self.patches);
        patches.sort_by_key(|patch| patch.priority.unwrap_or(0));
        let id = self.id.clone();
        let jar = self.jar.clone();
        let inherits_from = self.inherits_from.clone();
        let root = self.root;
        if root == Some(true) && inherits_from.is_none() && !patches.is_empty() {
            self.main_class = None;
            self.assets = None;
            self.asset_index = None;
            self.downloads.clear();
            self.libraries.clear();
            self.arguments = super::Arguments::default();
            self.minecraft_arguments = None;
            self.java_version = None;
            self.logging.clear();
            self.version_type = None;
        }
        for patch in patches {
            self = patch.into_launch_manifest().inherit(self);
        }
        self.id = id;
        self.jar = jar;
        self.inherits_from = inherits_from;
        self.root = root;
        self
    }
}
