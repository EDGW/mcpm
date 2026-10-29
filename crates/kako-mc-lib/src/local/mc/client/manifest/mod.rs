//! Client version manifest decoding, inspection and inheritance.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::local::modloader::{LoaderId, ModLoaderMeta};

use super::MinecraftModLoader;

mod assets;
mod launch;
mod library;
mod rules;

pub use assets::{AssetIndex, AssetIndexInfo, AssetObject};
pub use launch::{ClientLogging, JavaRequirement, LoggingFile};
pub use library::{
    DownloadInfo, LibraryCoordinateError, LibraryDownloads, LibraryExtract, MavenCoordinate,
    MinecraftLibrary,
};
pub use rules::{LibraryRule, LibraryRuleAction, LibraryRuleOs};

/// Loader-neutral fields from a client version JSON. Unknown fields are ignored.
/// Can also be deserialized directly for inspecting an in-memory profile.
///
/// # Implementations
/// - [`Self::id`]: manifest inspection, loader identification and inheritance.
/// - [`Self`]: internal `into_launch_manifest` normalization (`manifest/launch.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftVersionManifest {
    id: String,
    root: Option<bool>,
    version: Option<String>,
    hidden: Option<bool>,
    priority: Option<i32>,
    inherits_from: Option<String>,
    main_class: Option<String>,
    assets: Option<String>,
    asset_index: Option<AssetIndexInfo>,
    jar: Option<String>,
    #[serde(default)]
    downloads: std::collections::BTreeMap<String, DownloadInfo>,
    #[serde(default)]
    libraries: Vec<MinecraftLibrary>,
    #[serde(default)]
    arguments: Arguments,
    minecraft_arguments: Option<String>,
    java_version: Option<JavaRequirement>,
    #[serde(default)]
    logging: std::collections::BTreeMap<String, ClientLogging>,
    #[serde(rename = "type")]
    version_type: Option<String>,
    #[serde(default)]
    patches: Vec<MinecraftVersionManifest>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Arguments {
    #[serde(default)]
    game: Vec<Value>,
    #[serde(default)]
    jvm: Vec<Value>,
}

impl MinecraftVersionManifest {
    pub fn java_version(&self) -> Option<&JavaRequirement> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.java_version())
            .or(self.java_version.as_ref())
    }

    pub fn client_logging(&self) -> Option<&ClientLogging> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.client_logging())
            .or_else(|| self.logging.get("client"))
    }

    pub fn version_type(&self) -> Option<&str> {
        self.version_type.as_deref()
    }

    pub fn jvm_arguments(&self) -> &[Value] {
        &self.arguments.jvm
    }

    pub fn legacy_launch_arguments(&self) -> Option<&str> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.legacy_launch_arguments())
            .or(self.minecraft_arguments.as_deref())
    }
    /// Version directory supplying the client JAR after external inheritance resolution.
    pub fn jar(&self) -> Option<&str> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.jar())
            .or(self.jar.as_deref())
    }

    pub fn downloads(&self) -> &std::collections::BTreeMap<String, DownloadInfo> {
        &self.downloads
    }

    pub fn client_download(&self) -> Option<&DownloadInfo> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.client_download())
            .or_else(|| self.downloads.get("client"))
    }

    pub(super) fn default_jar(&mut self, directory_name: &str) {
        if self.jar.is_none() && self.inherits_from.is_none() {
            self.jar = Some(directory_name.to_owned());
        }
    }

    /// Asset set name used by legacy manifests.
    pub fn assets(&self) -> Option<&str> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.assets())
            .or(self.assets.as_deref())
    }

    /// Resolved asset index declaration, including download size and SHA-1.
    pub fn asset_index(&self) -> Option<&AssetIndexInfo> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.asset_index())
            .or(self.asset_index.as_ref())
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn inherits_from(&self) -> Option<&str> {
        self.inherits_from.as_deref()
    }
    pub fn main_class(&self) -> Option<&str> {
        self.ordered_patches()
            .into_iter()
            .rev()
            .find_map(|patch| patch.main_class())
            .or(self.main_class.as_deref())
    }
    /// Libraries in this profile and its HMCL patches, in ascending patch priority.
    /// Hidden patches still contribute libraries, as they do in HMCL resolution.
    pub fn libraries(&self) -> impl Iterator<Item = &MinecraftLibrary> {
        let mut libraries: Vec<_> = self.libraries.iter().collect();
        self.collect_patch_libraries(&mut libraries);
        libraries.into_iter()
    }
    fn collect_patch_libraries<'a>(&'a self, libraries: &mut Vec<&'a MinecraftLibrary>) {
        for patch in self.ordered_patches() {
            libraries.extend(&patch.libraries);
            patch.collect_patch_libraries(libraries);
        }
    }
    fn ordered_patches(&self) -> Vec<&Self> {
        let mut patches: Vec<_> = self.patches.iter().collect();
        patches.sort_by_key(|patch| patch.priority.unwrap_or(0));
        patches
    }
    /// HMCL patch declarations, retaining their document order.
    pub fn patches(&self) -> &[Self] {
        &self.patches
    }
    pub fn declared_version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden.unwrap_or(false)
    }
    /// Raw game arguments retain conditional objects rather than assuming their rules match.
    pub fn game_arguments(&self) -> &[Value] {
        &self.arguments.game
    }
    pub fn minecraft_arguments(&self) -> Option<&str> {
        self.minecraft_arguments.as_deref()
    }

    /// Runs the supplied loader implementations without referencing built-in loaders.
    pub fn mod_loaders(
        &self,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
    ) -> Result<Vec<MinecraftModLoader>> {
        let mut loaders: Vec<_> = loaders.values().collect();
        loaders.sort_by(|a, b| a.id().cmp(b.id()));
        let mut detected = Vec::new();
        for loader in loaders {
            let mut version = loader
                .instance()
                .try_identify_client(self)
                .with_context(|| {
                    format!("failed to inspect client {:?} for {}", self.id, loader.id())
                })?;
            // HMCL scans libraries first, then lets visible patch declarations
            // overwrite the entry with the same patch ID. Unknown IDs are ignored
            // here because callers explicitly select their supported loaders.
            for patch in &self.patches {
                if !patch.is_hidden() && patch.id == loader.id().as_str() {
                    version = patch.version.clone();
                }
            }
            if let Some(version) = version {
                detected.push(MinecraftModLoader {
                    loader: loader.clone(),
                    version,
                });
            }
        }
        Ok(detected)
    }

    pub(super) fn inherit(mut self, mut parent: Self) -> Self {
        // Retain inherited declarations: platform rules decide whether a child
        // overrides a parent library. Consumers select the last applicable identity.
        parent.libraries.extend(self.libraries);
        self.libraries = parent.libraries;
        parent.arguments.game.extend(self.arguments.game);
        self.arguments.game = parent.arguments.game;
        parent.arguments.jvm.extend(self.arguments.jvm);
        self.arguments.jvm = parent.arguments.jvm;
        self.java_version = self.java_version.or(parent.java_version);
        parent.logging.extend(self.logging);
        self.logging = parent.logging;
        self.version_type = self.version_type.or(parent.version_type);
        self.main_class = self.main_class.or(parent.main_class);
        self.jar = self.jar.or(parent.jar);
        parent.downloads.extend(self.downloads);
        self.downloads = parent.downloads;
        // A child selecting a different asset set must not retain its parent's index.
        if self.asset_index.is_none()
            && (self.assets.is_none()
                || self.assets == parent.assets
                || parent
                    .asset_index
                    .as_ref()
                    .is_some_and(|index| self.assets.as_deref() == Some(index.id.as_str())))
        {
            self.asset_index = parent.asset_index;
        }
        self.assets = self.assets.or(parent.assets);
        self.minecraft_arguments = self.minecraft_arguments.or(parent.minecraft_arguments);
        parent
            .patches
            .retain(|patch| !self.patches.iter().any(|child| child.id == patch.id));
        parent.patches.extend(self.patches);
        self.patches = parent.patches;
        self.inherits_from = None;
        self
    }
}
