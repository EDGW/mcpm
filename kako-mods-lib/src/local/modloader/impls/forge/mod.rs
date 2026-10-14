use std::path::Path;
use std::sync::{Arc, LazyLock};

use anyhow::Result;

use crate::local::{
    mcmod::{ModEntry, ModMeta},
    modloader::{ModLoader, ModLoaderMeta},
};

use super::{archive, fml, jarjar, version};

mod error;
pub use error::ForgeError;

/// Identifies mod declarations in `META-INF/mods.toml`.
/// Resolves version references from manifest attributes and document properties.
/// Ordinary version strings retain their declared values.
/// Documents explicitly requiring only the NeoForge platform are not recognized.
/// Declared JarJar children are recursively identified, including library-only wrappers.
#[derive(Debug, Default)]
pub struct ForgeLoader;

static METADATA: LazyLock<ModLoaderMeta> = LazyLock::new(|| ModLoaderMeta {
    id: "forge".into(),
    name: "Forge".into(),
    instance: Arc::new(ForgeLoader),
});

impl ModLoader for ForgeLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| ForgeError::OpenArchive {
            path: entry.path().to_owned(),
            source,
        })?;
        let root = read_archive(&mut archive, entry.path())?;
        let mods =
            archive::collect(&mut archive, root, entry.path(), read_archive).map_err(|source| {
                ForgeError::NestedIdentification {
                    path: entry.path().to_owned(),
                    source,
                }
            })?;
        if mods.is_empty() {
            return Ok(None);
        }
        Ok(Some(ModMeta {
            loader: self.metadata().clone(),
            mods,
        }))
    }
}

fn read_archive(archive: &mut archive::Archive, path: &Path) -> Result<archive::ArchiveNode> {
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
        Ok(fml::requires_platform(&contents, document, "neoforge")?
            && !fml::requires_platform(&contents, document, "forge")?)
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
        fml::parse(&contents, document, fml::DependencyFormat::Forge).map_err(|source| {
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
