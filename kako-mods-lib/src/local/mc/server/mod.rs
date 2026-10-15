//! Local server roots identified exclusively by `.mcpm/server.json`.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::local::modloader::{BUILTIN_LOADERS, LoaderInfo};

pub mod identify;
mod metadata;

pub use metadata::{MinecraftServerMetadata, ServerModLoader, ServerStartCommand};

/// A successfully loaded snapshot of a Minecraft server's declared metadata.
/// Loading does not inspect JARs or guarantee that the server can launch.
#[derive(Debug, Clone)]
pub struct MinecraftServer {
    path: PathBuf,
    name: String,
    metadata: MinecraftServerMetadata,
}

impl MinecraftServer {
    /// Reads the required metadata without creating files or guessing loader information.
    /// Paths are retained as supplied; `~` is not expanded.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, MinecraftServerError> {
        let path = path.into();
        let name = directory_name(&path)?;
        let manifest = path.join(".mcpm/server.json");
        let bytes = fs::read(&manifest).map_err(|source| MinecraftServerError::ReadMetadata {
            path: manifest.clone(),
            source,
        })?;
        let metadata = serde_json::from_slice(&bytes).map_err(|source| {
            MinecraftServerError::ParseMetadata {
                path: manifest,
                source,
            }
        })?;
        Ok(Self {
            path,
            name,
            metadata,
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
        let mut bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|source| MinecraftServerError::SerializeMetadata { source })?;
        // Validate public, caller-constructed metadata before writing anything.
        let metadata = serde_json::from_slice(&bytes).map_err(|source| {
            MinecraftServerError::ParseMetadata {
                path: manifest.clone(),
                source,
            }
        })?;
        bytes.push(b'\n');
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
        Ok(Self {
            path,
            name,
            metadata,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Server directory name, including when opened using `.` or `..`.
    /// Non-UTF-8 names are displayed with replacement characters.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolves the declared loader against the built-in loader registry.
    /// `None` means vanilla or unrecognized; declarations remain in `metadata()`.
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
