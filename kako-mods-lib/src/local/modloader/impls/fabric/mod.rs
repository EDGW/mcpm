use std::path::Path;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::local::{
    mcmod::{
        Dependency, DependencyConstraint, DependencyKind, DependencyReference, ModEntry, ModInfo,
        ModMeta,
    },
    modloader::{ModLoader, ModLoaderMeta},
};

use super::{archive, constraints, metadata};

mod error;
pub use error::FabricError;

/// Identifies declarations in `fabric.mod.json`.
/// Mod identifiers, names, and versions retain their declared string values.
/// Declared `jars[].file` children are recursively identified.
#[derive(Debug, Default)]
pub struct FabricLoader;

static METADATA: LazyLock<ModLoaderMeta> = LazyLock::new(|| ModLoaderMeta {
    id: "fabric".into(),
    name: "Fabric".into(),
    instance: Arc::new(FabricLoader),
});

impl ModLoader for FabricLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| FabricError::OpenArchive {
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
        .map_err(|source| FabricError::NestedIdentification {
            path: entry.path().to_owned(),
            source,
        })?;
        if !metadata.has_mods() {
            return Ok(None);
        }
        Ok(Some(metadata))
    }
}

pub(super) fn read_archive(
    archive: &mut archive::Archive,
    path: &Path,
) -> Result<archive::ArchiveNode> {
    const DOCUMENT: &str = "fabric.mod.json";
    let contents =
        archive::read(archive, DOCUMENT).map_err(|source| FabricError::ReadMetadata {
            path: path.to_owned(),
            document: DOCUMENT,
            source,
        })?;
    let Some(contents) = contents else {
        return Ok(archive::ArchiveNode::default());
    };
    let node = (|| -> Result<_> {
        Ok(archive::ArchiveNode {
            mods: vec![parse(&contents)?],
            jars: nested_paths(&contents)?,
        })
    })()
    .map_err(|source| FabricError::ParseMetadata {
        path: path.to_owned(),
        document: DOCUMENT,
        source,
    })?;
    Ok(node)
}

pub(super) fn nested_paths(contents: &str) -> Result<Vec<String>> {
    const DOCUMENT: &str = "fabric.mod.json";
    let data: Value = serde_json::from_str(contents)?;
    let Some(jars) = data.get("jars") else {
        return Ok(Vec::new());
    };
    metadata::array(jars, DOCUMENT, "jars")?
        .iter()
        .map(|jar| {
            metadata::object(jar, DOCUMENT, "jars[]")?;
            Ok(metadata::required_string(jar, "file", DOCUMENT)?.to_owned())
        })
        .collect()
}

pub(super) fn parse(contents: &str) -> Result<ModInfo> {
    let document = "fabric.mod.json";
    let data: Value = serde_json::from_str(contents)?;
    metadata::object(&data, document, "root")?;
    let modid = metadata::required_string(&data, "id", document)?.to_owned();
    let version = metadata::required_string(&data, "version", document)?.to_owned();
    let name = metadata::optional_string(&data, "name", document)?
        .unwrap_or(&modid)
        .to_owned();
    let mut dependencies = Vec::new();
    let schema = data
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Fabric v0 conflicts are hard incompatibilities and recommends are suggestions.
    let keys: &[(&str, DependencyKind)] = if schema == 0 {
        &[
            ("requires", DependencyKind::Required),
            ("recommends", DependencyKind::Suggested),
            ("conflicts", DependencyKind::Breaks),
        ]
    } else {
        &[
            ("depends", DependencyKind::Required),
            ("recommends", DependencyKind::Recommended),
            ("suggests", DependencyKind::Suggested),
            ("breaks", DependencyKind::Breaks),
            ("conflicts", DependencyKind::Conflicts),
        ]
    };
    for &(key, kind) in keys {
        if let Some(clauses) = data.get(key) {
            for (target, constraint) in metadata::object(clauses, document, key)? {
                dependencies.push(Dependency {
                    kind,
                    constraint: DependencyConstraint::Reference(DependencyReference {
                        id: target.clone(),
                        version_requirement: constraints::fabric_json(constraint).with_context(
                            || format!("{document}: invalid {key} constraint for {target}"),
                        )?,
                        qualifiers: Default::default(),
                    }),
                });
            }
        }
    }
    Ok(ModInfo {
        name,
        modid,
        version,
        dependencies,
    })
}
