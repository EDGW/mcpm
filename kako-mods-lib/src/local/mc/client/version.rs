use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
};

use anyhow::Result;
use thiserror::Error;

use crate::local::modloader::{LoaderId, ModLoaderMeta};

use super::{MinecraftDirectory, MinecraftModLoader, MinecraftVersionManifest};

/// A snapshot of a local profile's load result. `Normal` means its JSON and
/// inheritance resolved successfully, not that its game or loader can launch.
#[derive(Debug, Clone)]
pub enum MinecraftVersion {
    Normal(NormalVersion),
    Broken(BrokenVersion),
}

impl MinecraftVersion {
    pub(super) fn load(
        directory: &MinecraftDirectory,
        name: String,
    ) -> std::result::Result<Self, MinecraftVersionError> {
        let location = VersionLocation::new(directory, name)?;
        Ok(match load_manifest(&location) {
            Ok(manifest) => Self::Normal(NormalVersion {
                location,
                manifest: Box::new(manifest),
            }),
            Err(error) => Self::Broken(BrokenVersion {
                location,
                error: Arc::new(error),
            }),
        })
    }

    fn location(&self) -> &VersionLocation {
        match self {
            Self::Normal(version) => &version.location,
            Self::Broken(version) => &version.location,
        }
    }

    pub(super) fn root_path(&self) -> &std::path::Path {
        self.location().directory.path()
    }

    /// Directory name, which may differ from the JSON `id`.
    pub fn name(&self) -> &str {
        &self.location().name
    }
    pub fn path(&self) -> PathBuf {
        self.location().path()
    }
    pub fn manifest_path(&self) -> PathBuf {
        self.location().manifest_path()
    }

    /// Client JAR location, respecting `jar` aliases and inherited base versions.
    pub fn jar_path(&self) -> Result<PathBuf> {
        let manifest = self.manifest()?;
        let name = manifest.jar().unwrap_or(self.name());
        let location = VersionLocation::new(&self.location().directory, name.to_owned())?;
        Ok(location.path().join(format!("{name}.jar")))
    }

    /// Returns cached resolved metadata, or the captured load error.
    /// No filesystem access occurs. Match `Normal` to access metadata infallibly.
    pub fn manifest(&self) -> Result<&MinecraftVersionManifest> {
        match self {
            Self::Normal(version) => Ok(version.manifest()),
            Self::Broken(version) => Err(anyhow::Error::new(Arc::clone(&version.error))),
        }
    }

    /// Applies HMCL's main-class heuristic to cached metadata.
    /// Broken profiles return their captured error; they are never treated as vanilla.
    pub fn is_modded(&self) -> Result<bool> {
        match self {
            Self::Normal(version) => Ok(version.is_modded()),
            Self::Broken(version) => Err(anyhow::Error::new(Arc::clone(&version.error))),
        }
    }

    /// Inspects cached declarations using only the supplied loaders, in ID order.
    /// An empty result does not guarantee vanilla or that libraries are installed.
    /// Load and loader-recognition errors are returned without a partial result.
    pub fn mod_loaders(
        &self,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
    ) -> Result<Vec<MinecraftModLoader>> {
        self.manifest()?.mod_loaders(loaders)
    }
}

/// A successfully loaded profile with resolved, immutable metadata.
#[derive(Debug, Clone)]
pub struct NormalVersion {
    location: VersionLocation,
    manifest: Box<MinecraftVersionManifest>,
}

impl NormalVersion {
    pub fn name(&self) -> &str {
        &self.location.name
    }

    pub fn path(&self) -> PathBuf {
        self.location.path()
    }

    pub fn manifest(&self) -> &MinecraftVersionManifest {
        &self.manifest
    }

    /// HMCL's `isModded` heuristic; independent of specific loader recognition.
    pub fn is_modded(&self) -> bool {
        self.manifest.main_class().is_some_and(|main| {
            main == "net.minecraft.launchwrapper.Launch"
                || [
                    "net.minecraftforge",
                    "net.neoforged",
                    "top.outlands",
                    "net.fabricmc",
                    "org.quiltmc",
                    "cpw.mods",
                ]
                .iter()
                .any(|prefix| main.starts_with(prefix))
        })
    }

    pub fn mod_loaders(
        &self,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
    ) -> Result<Vec<MinecraftModLoader>> {
        self.manifest.mod_loaders(loaders)
    }
}

/// A profile whose manifest or inheritance could not be loaded.
#[derive(Debug, Clone)]
pub struct BrokenVersion {
    location: VersionLocation,
    error: Arc<MinecraftVersionError>,
}

impl BrokenVersion {
    pub fn name(&self) -> &str {
        &self.location.name
    }

    pub fn path(&self) -> PathBuf {
        self.location.path()
    }

    /// The original typed error, including its filesystem/parser source.
    pub fn error(&self) -> &MinecraftVersionError {
        &self.error
    }
}

#[derive(Debug, Clone)]
struct VersionLocation {
    directory: MinecraftDirectory,
    name: String,
}

impl VersionLocation {
    fn new(
        directory: &MinecraftDirectory,
        name: String,
    ) -> std::result::Result<Self, MinecraftVersionError> {
        if name.is_empty() || matches!(name.as_str(), "." | "..") || name.contains(['/', '\\']) {
            return Err(MinecraftVersionError::InvalidName { name });
        }
        Ok(Self {
            directory: directory.clone(),
            name,
        })
    }
    fn path(&self) -> PathBuf {
        self.directory.path().join("versions").join(&self.name)
    }
    fn manifest_path(&self) -> PathBuf {
        self.path().join(format!("{}.json", self.name))
    }
}

fn load_manifest(
    location: &VersionLocation,
) -> std::result::Result<MinecraftVersionManifest, MinecraftVersionError> {
    let mut chain = Vec::new();
    let mut names = HashSet::new();
    let mut current = location.clone();
    loop {
        if !names.insert(current.name.clone()) {
            return Err(MinecraftVersionError::InheritanceCycle { name: current.name });
        }
        let path = current.manifest_path();
        let bytes = fs::read(&path).map_err(|source| MinecraftVersionError::ReadManifest {
            path: path.clone(),
            source,
        })?;
        let mut manifest: MinecraftVersionManifest = serde_json::from_slice(&bytes)
            .map_err(|source| MinecraftVersionError::ParseManifest { path, source })?;
        manifest.default_jar(&current.name);
        let parent = manifest.inherits_from().map(str::to_owned);
        chain.push(manifest);
        match parent {
            Some(name) => current = VersionLocation::new(&location.directory, name)?,
            None => break,
        }
    }
    let mut resolved = chain.pop().expect("at least one manifest was read");
    while let Some(child) = chain.pop() {
        resolved = child.inherit(resolved);
    }
    Ok(resolved)
}

#[derive(Debug, Error)]
pub enum MinecraftVersionError {
    #[error("invalid Minecraft version directory name {name:?}")]
    InvalidName { name: String },
    #[error("Minecraft version inheritance cycle at {name:?}")]
    InheritanceCycle { name: String },
    #[error("failed to read Minecraft version manifest {path:?}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse Minecraft version manifest {path:?}")]
    ParseManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}
