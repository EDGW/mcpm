use std::path::{Path, PathBuf};

use thiserror::Error;

use super::{
    manifest::{DownloadInfo, LibraryCoordinateError, MinecraftLibrary},
    path::{self, ResourcePathError},
    rules::{LibraryRuleError, RuleEnvironment},
};

/// Read-only access to the shared `libraries/` repository. It may not exist yet.
#[derive(Debug, Clone)]
pub struct LibrariesDirectory {
    path: PathBuf,
}

/// A selected artifact or native archive, including verification/download metadata.
#[derive(Debug, Clone)]
pub struct LibraryFile {
    pub kind: LibraryFileKind,
    pub name: String,
    pub classifier: Option<String>,
    pub path: PathBuf,
    pub download: DownloadInfo,
    /// Alternative SHA-1 values declared by legacy library metadata.
    pub checksums: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFileKind {
    Artifact,
    Native,
}

impl LibrariesDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file_path(&self, relative: &str) -> Result<PathBuf, ResourcePathError> {
        path::within(&self.path, relative)
    }

    /// Resolves only artifacts and natives required by the supplied environment.
    /// Other classifiers (sources, other platforms, etc.) are not required.
    /// No downloads or native extraction are performed.
    pub fn resolve(
        &self,
        library: &MinecraftLibrary,
        rules: &RuleEnvironment,
    ) -> Result<Vec<LibraryFile>, LibraryError> {
        if library.clientreq == Some(false) || !rules.allows(&library.rules)? {
            return Ok(Vec::new());
        }
        let coordinate = library.coordinate()?;
        let mut files = Vec::new();
        let artifact = match &library.downloads {
            Some(downloads) if downloads.artifact.is_none() && library.natives.is_empty() => {
                return Err(LibraryError::MissingArtifact {
                    library: library.name.clone(),
                });
            }
            Some(downloads) => downloads.artifact.clone(),
            None if library.natives.is_empty() => Some(DownloadInfo::default()),
            None => None,
        };
        if let Some(artifact) = artifact {
            files.push(self.resolve_file(
                library,
                coordinate.classifier.clone(),
                artifact,
                true,
            )?);
        }
        if let Some(classifier) = library.natives.get(&rules.platform.os) {
            if classifier.contains("${arch}") && !matches!(rules.platform.bits, 32 | 64) {
                return Err(LibraryError::InvalidBitness {
                    bits: rules.platform.bits,
                });
            }
            let classifier = classifier.replace("${arch}", &rules.platform.bits.to_string());
            let download = match &library.downloads {
                Some(downloads) => {
                    downloads
                        .classifiers
                        .get(&classifier)
                        .cloned()
                        .ok_or_else(|| LibraryError::MissingClassifier {
                            library: library.name.clone(),
                            classifier: classifier.clone(),
                        })?
                }
                None => DownloadInfo::default(),
            };
            files.push(self.resolve_file(library, Some(classifier), download, false)?);
        }
        Ok(files)
    }

    fn resolve_file(
        &self,
        library: &MinecraftLibrary,
        classifier: Option<String>,
        mut download: DownloadInfo,
        primary: bool,
    ) -> Result<LibraryFile, LibraryError> {
        let mut coordinate = library.coordinate()?;
        coordinate.classifier = classifier.clone();
        let relative = match &download.path {
            Some(path) => path.clone(),
            None => coordinate.path()?.to_string_lossy().replace('\\', "/"),
        };
        let path = self.file_path(&relative)?;
        if download.url.is_none() {
            download.url = Some(format!(
                "{}/{}",
                library
                    .url
                    .as_deref()
                    .unwrap_or("https://libraries.minecraft.net")
                    .trim_end_matches('/'),
                relative
            ));
        }
        download.path = Some(relative);
        let checksums = if primary && download.sha1.is_none() {
            library.checksums.clone()
        } else {
            Vec::new()
        };
        Ok(LibraryFile {
            kind: if primary {
                LibraryFileKind::Artifact
            } else {
                LibraryFileKind::Native
            },
            name: library.name.clone(),
            classifier,
            path,
            download,
            checksums,
        })
    }
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("library {library:?} declares downloads without an artifact or native selection")]
    MissingArtifact { library: String },
    #[error(transparent)]
    Coordinate(#[from] LibraryCoordinateError),
    #[error(transparent)]
    Path(#[from] ResourcePathError),
    #[error(transparent)]
    Rule(#[from] LibraryRuleError),
    #[error("library {library:?} does not declare native classifier {classifier:?}")]
    MissingClassifier { library: String, classifier: String },
    #[error("invalid native architecture bitness {bits}; expected 32 or 64")]
    InvalidBitness { bits: u8 },
}
