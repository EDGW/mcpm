use std::path::Path;
use std::sync::{Arc, LazyLock};

use anyhow::Result;

use crate::local::{
    mcmod::{ModEntry, ModMeta},
    modloader::{ModLoader, ModLoaderMeta},
};

use super::{archive, fml, jarjar, version};

mod error;
pub use error::NeoForgeError;

/// Identifies mod declarations in `META-INF/neoforge.mods.toml`.
/// Resolves version references from manifest attributes and document properties.
/// Ordinary version strings retain their declared values.
/// Also recognizes older `mods.toml` documents explicitly requiring NeoForge.
/// Declared JarJar children are recursively identified, including library-only wrappers.
#[derive(Debug, Default)]
pub struct NeoForgeLoader;

static METADATA: LazyLock<ModLoaderMeta> = LazyLock::new(|| {
    ModLoaderMeta::new(
        "neoforge".into(),
        "NeoForge".into(),
        Arc::new(NeoForgeLoader),
    )
});

impl ModLoader for NeoForgeLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| NeoForgeError::OpenArchive {
            path: entry.path().to_owned(),
            source,
        })?;
        let root = read_archive(&mut archive, entry.path())?;
        let metadata = archive::collect(
            &mut archive,
            root,
            entry.path(),
            self.metadata(),
            read_archive,
        )
        .map_err(|source| NeoForgeError::NestedIdentification {
            path: entry.path().to_owned(),
            source,
        })?;
        if !metadata.has_mods() {
            return Ok(None);
        }
        Ok(Some(metadata))
    }
}

fn read_archive(archive: &mut archive::Archive, path: &Path) -> Result<archive::ArchiveNode> {
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
            fml::requires_platform(&contents, document, "neoforge").map_err(|source| {
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
        fml::DependencyFormat::NeoForgeLegacy
    } else {
        fml::DependencyFormat::NeoForge
    };
    let mut mods =
        fml::parse(&contents, document, format).map_err(|source| NeoForgeError::ParseMetadata {
            path: path.to_owned(),
            document,
            source,
        })?;
    for info in &mut mods {
        info.version = version::resolve(&info.version, &contents, || {
            archive::read(archive, "META-INF/MANIFEST.MF").map_err(|source| {
                NeoForgeError::ReadMetadata {
                    path: path.to_owned(),
                    document: "META-INF/MANIFEST.MF",
                    source,
                }
                .into()
            })
        })
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
