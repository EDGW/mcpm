//! NeoForge mod declarations, JarJar declarations and version expansion.
//! Supports native neoforge.mods.toml and old mods.toml explicitly requiring NeoForge.

use std::path::Path;

use super::super::super::utils::archive;
use super::super::{dependency::DependencyFormat, jarjar, metadata, version};
use super::NeoForgeError;
use anyhow::Result;

pub(super) fn read_archive(
    archive: &mut archive::Archive,
    path: &Path,
) -> Result<archive::ArchiveNode> {
    let declaration =
        archive::read(archive, jarjar::DOCUMENT).map_err(|source| NeoForgeError::ReadMetadata {
            path: path.to_owned(),
            document: jarjar::DOCUMENT,
            source,
        })?;
    let jars = declaration
        .as_deref()
        .map(jarjar::paths)
        .transpose()
        .map_err(|source| NeoForgeError::ParseMetadata {
            path: path.to_owned(),
            document: jarjar::DOCUMENT,
            source,
        })?
        .unwrap_or_default();

    let mut document = "META-INF/neoforge.mods.toml";
    let contents =
        archive::read(archive, document).map_err(|source| NeoForgeError::ReadMetadata {
            path: path.to_owned(),
            document,
            source,
        })?;
    let contents = if let Some(contents) = contents {
        contents
    } else {
        document = "META-INF/mods.toml";
        let contents =
            archive::read(archive, document).map_err(|source| NeoForgeError::ReadMetadata {
                path: path.to_owned(),
                document,
                source,
            })?;
        let Some(contents) = contents else {
            return Ok(archive::ArchiveNode { mods: vec![], jars });
        };
        let recognized =
            metadata::requires_platform(&contents, document, "neoforge").map_err(|source| {
                NeoForgeError::ParseMetadata {
                    path: path.to_owned(),
                    document,
                    source,
                }
            })?;
        if !recognized {
            return Ok(archive::ArchiveNode { mods: vec![], jars });
        }
        contents
    };

    let format = if document == "META-INF/mods.toml" {
        DependencyFormat::NeoForgeLegacy
    } else {
        DependencyFormat::NeoForge
    };
    let mut mods = metadata::parse(&contents, document, format).map_err(|source| {
        NeoForgeError::ParseMetadata {
            path: path.to_owned(),
            document,
            source,
        }
    })?;
    for info in &mut mods {
        info.version = version::resolve(
            version::VersionFormat::NeoForge,
            &info.version,
            &contents,
            || {
                archive::read(archive, "META-INF/MANIFEST.MF").map_err(|source| {
                    NeoForgeError::ReadMetadata {
                        path: path.to_owned(),
                        document: "META-INF/MANIFEST.MF",
                        source,
                    }
                    .into()
                })
            },
        )
        .map_err(|source| NeoForgeError::ResolveVersion {
            path: path.to_owned(),
            document,
            modid: info.modid.clone(),
            expression: info.version.clone(),
            source,
        })?;
    }
    Ok(archive::ArchiveNode { mods, jars })
}
