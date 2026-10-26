//! Local server roots identified exclusively by `.mcpm/server.json`.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::local::{
    mcmod::{ModContainer, ModDirectory},
    modloader::{BUILTIN_LOADERS, LoaderInfo},
    runtime::RuntimeEnvironment,
};

pub mod config;
pub mod identify;
pub mod launch;
pub mod stash;

pub use launch::{ServerLaunch, ServerOptionError};
mod metadata;
mod script_path;

pub use script_path::ServerScriptError;

pub use metadata::{
    MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader, ServerStartCommand,
};

/// A successfully loaded snapshot of a Minecraft server's declared metadata.
/// Loading does not inspect JARs or guarantee that the server can launch.
#[derive(Debug, Clone)]
pub struct MinecraftServer {
    path: PathBuf,
    name: String,
    metadata: MinecraftServerMetadata,
    mods: Option<ModContainer>,
    runtime: RuntimeEnvironment,
}

impl MinecraftServer {
    /// Validates a prospective registration without creating directories or reading old metadata.
    fn registration_snapshot(
        path: PathBuf,
        metadata: MinecraftServerMetadata,
    ) -> Result<Self, MinecraftServerError> {
        let name = directory_name(&path)?;
        let mods = mod_container(&path, &metadata, false)?;
        let runtime = server_runtime(&path);
        Ok(Self {
            path,
            name,
            metadata,
            mods,
            runtime,
        })
    }

    /// Reads and validates metadata, then creates a missing `mods` directory for
    /// a recognized built-in loader. Vanilla and unknown loaders create no directory.
    /// Paths are retained as supplied; `~` is not expanded.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, MinecraftServerError> {
        Self::open(path.into(), true)
    }

    /// Opens a server without creating a missing `mods` directory. Metadata and
    /// loader information remain available; `mods()` is `None` when it is absent.
    pub fn inspect(path: impl Into<PathBuf>) -> Result<Self, MinecraftServerError> {
        Self::open(path.into(), false)
    }

    fn open(path: PathBuf, create_mods: bool) -> Result<Self, MinecraftServerError> {
        let name = directory_name(&path)?;
        let manifest = path.join(".mcpm/server.json");
        let bytes = fs::read(&manifest).map_err(|source| MinecraftServerError::ReadMetadata {
            path: manifest.clone(),
            source,
        })?;
        let metadata = metadata::decode(&manifest, &bytes)?;
        validate_start_script(&path, &metadata)?;
        let mods = mod_container(&path, &metadata, create_mods)?;
        let runtime = server_runtime(&path);
        Ok(Self {
            path,
            name,
            metadata,
            mods,
            runtime,
        })
    }

    /// Records metadata in an existing server root and returns its loaded snapshot.
    /// Creates `.mcpm` as needed and refuses to overwrite existing `server.json`.
    /// This only registers the server; it does not install server software.
    pub fn create(
        path: impl Into<PathBuf>,
        metadata: MinecraftServerMetadata,
    ) -> Result<Self, MinecraftServerError> {
        Self::write(path.into(), metadata, false)
    }

    /// Atomically replaces metadata after validation. Failed writes preserve the old file.
    pub fn replace(
        path: impl Into<PathBuf>,
        metadata: MinecraftServerMetadata,
    ) -> Result<Self, MinecraftServerError> {
        Self::write(path.into(), metadata, true)
    }

    fn write(
        path: PathBuf,
        metadata: MinecraftServerMetadata,
        overwrite: bool,
    ) -> Result<Self, MinecraftServerError> {
        let name = directory_name(&path)?;
        let manifest = path.join(".mcpm/server.json");
        let (metadata, bytes) = metadata::encode(&path, &metadata)?;
        let _lock = config::ConfigurationLock::acquire(&path, true)
            .map_err(|source| MinecraftServerError::Configuration { source })?;
        config::ConfigurationLock::require_clean(&path)
            .map_err(|source| MinecraftServerError::Configuration { source })?;
        // Validate the optional mod directory before changing registration metadata.
        let mods = mod_container(&path, &metadata, true)?;
        let write = || -> io::Result<()> {
            let directory = path.join(".mcpm");
            fs::create_dir_all(&directory)?;
            let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
            temporary.write_all(&bytes)?;
            if overwrite {
                temporary.persist(&manifest).map_err(|error| error.error)?;
            } else {
                temporary
                    .persist_noclobber(&manifest)
                    .map_err(|error| error.error)?;
            }
            Ok(())
        };
        write().map_err(|source| MinecraftServerError::WriteMetadata {
            path: manifest,
            source,
        })?;
        let runtime = server_runtime(&path);
        Ok(Self {
            path,
            name,
            metadata,
            mods,
            runtime,
        })
    }

    /// Global resources except for the server-local stash content cache.
    /// Accessing the environment does not open any cache.
    pub fn runtime(&self) -> &RuntimeEnvironment {
        &self.runtime
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Server directory name, including when opened using `.` or `..`.
    /// Non-UTF-8 names are displayed with replacement characters.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolves the declared loader against the complete [`BUILTIN_LOADERS`] registry.
    /// `None` means vanilla or unrecognized; declarations remain in `metadata()`.
    /// All loaders must be registered there; behavior for unregistered loaders
    /// is undefined by the library's API contract.
    pub fn loader(&self) -> Option<LoaderInfo> {
        let declaration = self.metadata.loader.as_ref()?;
        let loader = BUILTIN_LOADERS.get(declaration.loader_type.as_str())?;
        Some(LoaderInfo {
            loader: loader.clone(),
            version: declaration.version.clone(),
        })
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.path.join(".mcpm/server.json")
    }

    /// Cached declarations. Call `load` again to read changes from disk.
    pub fn metadata(&self) -> &MinecraftServerMetadata {
        &self.metadata
    }

    /// Mod directory associated with the declared built-in loader at load time.
    /// Returns `None` for vanilla or unknown loaders.
    /// Entries are scanned afresh by the container.
    pub fn mods(&self) -> Option<&ModContainer> {
        self.mods.as_ref()
    }
}

fn server_runtime(path: &Path) -> RuntimeEnvironment {
    RuntimeEnvironment::new().with_stash_cache(
        crate::local::cache::mods::ModsCacheSelection::Directory {
            path: stash::cache_path(path),
        },
    )
}

fn validate_start_script(
    root: &Path,
    metadata: &MinecraftServerMetadata,
) -> Result<(), MinecraftServerError> {
    if let Some(start) = &metadata.start {
        script_path::validate(root, &start.script)
            .map_err(|source| MinecraftServerError::InvalidStartScript { source })?;
    }
    Ok(())
}

fn mod_container(
    root: &Path,
    metadata: &MinecraftServerMetadata,
    create_missing: bool,
) -> Result<Option<ModContainer>, MinecraftServerError> {
    let Some(declaration) = &metadata.loader else {
        return Ok(None);
    };
    let Some(loader) = BUILTIN_LOADERS.get(declaration.loader_type.as_str()) else {
        return Ok(None);
    };
    let path = root.join("mods");
    // Create only after metadata validation and built-in loader resolution.
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create_missing {
                return Ok(None);
            }
            match fs::create_dir(&path) {
                Ok(()) => {}
                // Another caller may have created the entry; validate it below.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(MinecraftServerError::CreateModDirectory { path, source });
                }
            }
        }
        Err(source) => {
            return Err(MinecraftServerError::OpenModDirectory {
                path,
                source: source.into(),
            });
        }
        Ok(_) => {}
    }
    let dir = ModDirectory::new(path.clone())
        .map_err(|source| MinecraftServerError::OpenModDirectory { path, source })?;
    Ok(Some(ModContainer::new(
        LoaderInfo {
            loader: loader.clone(),
            version: declaration.version.clone(),
        },
        dir,
    )))
}

fn directory_name(path: &Path) -> Result<String, MinecraftServerError> {
    let metadata = fs::metadata(path).map_err(|source| MinecraftServerError::InspectDirectory {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(MinecraftServerError::NotDirectory {
            path: path.to_owned(),
        });
    }
    let resolved =
        fs::canonicalize(path).map_err(|source| MinecraftServerError::InspectDirectory {
            path: path.to_owned(),
            source,
        })?;
    Ok(path
        .file_name()
        .or_else(|| resolved.file_name())
        .unwrap_or(resolved.as_os_str())
        .to_string_lossy()
        .into_owned())
}

#[derive(Debug, Error)]
pub enum MinecraftServerError {
    #[error("server configuration cannot be changed")]
    Configuration {
        #[source]
        source: anyhow::Error,
    },
    #[error("invalid Minecraft server start script")]
    InvalidStartScript {
        #[source]
        source: ServerScriptError,
    },
    #[error("failed to create Minecraft server mod directory {path:?}")]
    CreateModDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to open Minecraft server mod directory {path:?}")]
    OpenModDirectory {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to inspect Minecraft server directory {path:?}")]
    InspectDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Minecraft server root {path:?} is not a directory")]
    NotDirectory { path: PathBuf },
    #[error("failed to read Minecraft server metadata {path:?}")]
    ReadMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid Minecraft server metadata {path:?}")]
    ParseMetadata {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize Minecraft server metadata")]
    SerializeMetadata {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write Minecraft server metadata {path:?}")]
    WriteMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
