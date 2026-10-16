//! Forge mods.toml recognition, JarJar declarations and version expansion.
//! Old metadata explicitly requiring only NeoForge is excluded.

use std::path::Path;

use super::super::super::utils::archive;
use super::super::{dependency::DependencyFormat, jarjar, metadata, version};
use super::ForgeError;
use anyhow::Result;

pub(super) fn read_archive(
    archive: &mut archive::Archive,
    path: &Path,
) -> Result<archive::ArchiveNode> {
    let declaration =
        archive::read(archive, jarjar::DOCUMENT).map_err(|source| ForgeError::ReadMetadata {
            path: path.to_owned(),
            document: jarjar::DOCUMENT,
            source,
        })?;
    let jars = declaration
        .as_deref()
        .map(jarjar::paths)
        .transpose()
        .map_err(|source| ForgeError::ParseMetadata {
            path: path.to_owned(),
            document: jarjar::DOCUMENT,
            source,
        })?
        .unwrap_or_default();

    const DOCUMENT: &str = "META-INF/mods.toml";
    let document = DOCUMENT;
    let contents = archive::read(archive, document).map_err(|source| ForgeError::ReadMetadata {
        path: path.to_owned(),
        document,
        source,
    })?;
    let Some(contents) = contents else {
        return Ok(archive::ArchiveNode { mods: vec![], jars });
    };
    let belongs_to_neoforge = (|| -> Result<bool> {
        Ok(
            metadata::requires_platform(&contents, document, "neoforge")?
                && !metadata::requires_platform(&contents, document, "forge")?,
        )
    })()
    .map_err(|source| ForgeError::ParseMetadata {
        path: path.to_owned(),
        document,
        source,
    })?;
    if belongs_to_neoforge {
        return Ok(archive::ArchiveNode { mods: vec![], jars });
    }

    let mut mods =
        metadata::parse(&contents, document, DependencyFormat::Forge).map_err(|source| {
            ForgeError::ParseMetadata {
                path: path.to_owned(),
                document,
                source,
            }
        })?;
    for info in &mut mods {
        info.version = version::resolve(&info.version, &contents, || {
            archive::read(archive, "META-INF/MANIFEST.MF").map_err(|source| {
                ForgeError::ReadMetadata {
                    path: path.to_owned(),
                    document: "META-INF/MANIFEST.MF",
                    source,
                }
                .into()
            })
        })
        .map_err(|source| ForgeError::ResolveVersion {
            path: path.to_owned(),
            document,
            modid: info.modid.clone(),
            expression: info.version.clone(),
            source,
        })?;
    }
    Ok(archive::ArchiveNode { mods, jars })
}
