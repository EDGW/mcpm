//! Sole local registration codec: explicit V4 identities and read-only V1–V3 adaptation.

use std::{io::Write, path::Path};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::MinecraftServerError;

mod v3;

/// Format emitted for newly persisted local server metadata.
pub const SERVER_METADATA_VERSION: u32 = 4;

/// Current in-memory registration. Legacy documents normalize to this format on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MinecraftServerMetadata {
    pub version: u32,
    pub minecraft_version: String,
    pub loader: ServerLoader,
}

impl<'de> Deserialize<'de> for MinecraftServerMetadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        DecodedMetadata::deserialize(deserializer).map(|document| document.metadata)
    }
}

/// Declared server identity, independent of installed loader implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerLoader {
    Vanilla,
    Unrecognized,
    Modded(ServerModLoader),
}

impl ServerLoader {
    /// A concrete declaration may be retained even when this build cannot resolve it.
    pub fn declaration(&self) -> Option<&ServerModLoader> {
        match self {
            Self::Modded(loader) => Some(loader),
            Self::Vanilla | Self::Unrecognized => None,
        }
    }
}

impl Serialize for ServerLoader {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (loader_type, version) = match self {
            Self::Vanilla => ("vanilla", None),
            Self::Unrecognized => ("unrecognized", None),
            Self::Modded(loader) => {
                if matches!(loader.loader_type.as_str(), "vanilla" | "unrecognized") {
                    return Err(serde::ser::Error::custom(
                        "reserved loader identities cannot carry a loader version",
                    ));
                }
                (loader.loader_type.as_str(), Some(loader.version.as_str()))
            }
        };
        #[derive(Serialize)]
        struct Document<'a> {
            #[serde(rename = "type")]
            loader_type: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<&'a str>,
        }
        Document {
            loader_type,
            version,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ServerLoader {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Document {
            #[serde(rename = "type", deserialize_with = "nonempty_string")]
            loader_type: String,
            #[serde(default)]
            version: Option<String>,
        }
        let document = Document::deserialize(deserializer)?;
        match document.loader_type.as_str() {
            "vanilla" | "unrecognized" => {
                if document.version.is_some() {
                    return Err(serde::de::Error::custom(
                        "vanilla and unrecognized do not have a loader version",
                    ));
                }
                Ok(if document.loader_type == "vanilla" {
                    Self::Vanilla
                } else {
                    Self::Unrecognized
                })
            }
            _ => {
                let version = document
                    .version
                    .filter(|version| !version.trim().is_empty())
                    .ok_or_else(|| serde::de::Error::custom("loader version must be nonempty"))?;
                Ok(Self::Modded(ServerModLoader {
                    loader_type: document.loader_type,
                    version,
                }))
            }
        }
    }
}

/// A concrete loader identity and version; no implementation is instantiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerModLoader {
    #[serde(rename = "type", deserialize_with = "nonempty_string")]
    pub loader_type: String,
    #[serde(deserialize_with = "nonempty_string")]
    pub version: String,
}

/// The source format remains separate from the normalized runtime model.
pub(super) struct DecodedMetadata {
    pub(super) metadata: MinecraftServerMetadata,
    pub(super) source_version: u32,
}

impl<'de> Deserialize<'de> for DecodedMetadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let source_version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or_else(|| {
                serde::de::Error::custom("server metadata version must be an unsigned integer")
            })?;
        let metadata = match source_version {
            1..=3 => {
                let legacy: v3::MinecraftServerMetadataV3 =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                let loader = match legacy.loader {
                    None => ServerLoader::Unrecognized,
                    Some(loader) => match loader.loader_type.as_str() {
                        "vanilla" => ServerLoader::Vanilla,
                        "unrecognized" => ServerLoader::Unrecognized,
                        _ => ServerLoader::Modded(ServerModLoader {
                            loader_type: loader.loader_type,
                            version: loader.version,
                        }),
                    },
                };
                MinecraftServerMetadata {
                    version: SERVER_METADATA_VERSION,
                    minecraft_version: legacy.minecraft_version,
                    loader,
                }
            }
            SERVER_METADATA_VERSION => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Document {
                    version: u32,
                    #[serde(deserialize_with = "nonempty_string")]
                    minecraft_version: String,
                    loader: ServerLoader,
                }
                let document: Document =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                MinecraftServerMetadata {
                    version: document.version,
                    minecraft_version: document.minecraft_version,
                    loader: document.loader,
                }
            }
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "unsupported server metadata version {source_version}; expected 1, 2, 3 or {SERVER_METADATA_VERSION}"
                )));
            }
        };
        Ok(Self {
            metadata,
            source_version,
        })
    }
}

pub(super) fn decode(path: &Path, bytes: &[u8]) -> Result<DecodedMetadata, MinecraftServerError> {
    serde_json::from_slice(bytes).map_err(|source| MinecraftServerError::ParseMetadata {
        path: path.to_owned(),
        source,
    })
}

/// Validates current public inputs through the same schema used for file reads.
pub(super) fn encode(
    root: &Path,
    metadata: &MinecraftServerMetadata,
) -> Result<(MinecraftServerMetadata, Vec<u8>), MinecraftServerError> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|source| MinecraftServerError::SerializeMetadata { source })?;
    let metadata = decode(&root.join(".mcpm/server.json"), &bytes)?.metadata;
    let mut bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|source| MinecraftServerError::SerializeMetadata { source })?;
    bytes.push(b'\n');
    Ok((metadata, bytes))
}

/// Publishes only registration bytes. The caller holds the configuration lock.
pub(super) fn publish(
    root: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<(), MinecraftServerError> {
    let manifest = root.join(".mcpm/server.json");
    let write = || -> std::io::Result<()> {
        let mut temporary = tempfile::NamedTempFile::new_in(root.join(".mcpm"))?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
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
    })
}

fn nonempty_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom("expected a nonempty string"));
    }
    Ok(value)
}
