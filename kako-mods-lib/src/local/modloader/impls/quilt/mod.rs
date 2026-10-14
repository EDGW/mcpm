use std::path::Path;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::local::{
    mcmod::{DependencyConstraint, ModDependency, ModEntry, ModInfo, ModMeta, VersionConstraint},
    modloader::{ModLoader, ModLoaderMeta},
};

use super::{archive, constraints, fabric, metadata};

mod error;
pub use error::QuiltError;

/// Identifies `quilt.mod.json`, preferring it over Fabric-compatible metadata.
/// When no Quilt document exists, `fabric.mod.json` is read using Fabric rules.
/// Recognition reports declarations rather than guaranteeing game compatibility.
/// Native `quilt_loader.jars` and Fabric-compatible `jars[].file` are followed recursively.
#[derive(Debug, Default)]
pub struct QuiltLoader;

static METADATA: LazyLock<ModLoaderMeta> = LazyLock::new(|| ModLoaderMeta {
    id: "quilt".into(),
    name: "Quilt".into(),
    instance: Arc::new(QuiltLoader),
});

impl ModLoader for QuiltLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &METADATA
    }

    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>> {
        let mut archive = archive::open(entry).map_err(|source| QuiltError::OpenArchive {
            path: entry.path().to_owned(),
            source,
        })?;
        let root = read_archive(&mut archive, entry.path())?;
        let mods =
            archive::collect(&mut archive, root, entry.path(), read_archive).map_err(|source| {
                QuiltError::NestedIdentification {
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
    let native =
        archive::read(archive, "quilt.mod.json").map_err(|source| QuiltError::ReadMetadata {
            path: path.to_owned(),
            document: "quilt.mod.json",
            source,
        })?;
    let (document, contents) = if let Some(contents) = native {
        ("quilt.mod.json", contents)
    } else {
        let compatible = archive::read(archive, "fabric.mod.json").map_err(|source| {
            QuiltError::ReadMetadata {
                path: path.to_owned(),
                document: "fabric.mod.json",
                source,
            }
        })?;
        let Some(contents) = compatible else {
            return Ok(archive::ArchiveNode::default());
        };
        ("fabric.mod.json", contents)
    };
    (|| -> Result<_> {
        if document == "quilt.mod.json" {
            Ok(archive::ArchiveNode {
                mods: vec![parse(&contents)?],
                jars: nested_paths(&contents)?,
            })
        } else {
            Ok(archive::ArchiveNode {
                mods: vec![fabric::parse(&contents)?],
                jars: fabric::nested_paths(&contents)?,
            })
        }
    })()
    .map_err(|source| {
        QuiltError::ParseMetadata {
            path: path.to_owned(),
            document,
            source,
        }
        .into()
    })
}

fn nested_paths(contents: &str) -> Result<Vec<String>> {
    const DOCUMENT: &str = "quilt.mod.json";
    let data: Value = serde_json::from_str(contents)?;
    let Some(jars) = data
        .get("quilt_loader")
        .and_then(|loader| loader.get("jars"))
    else {
        return Ok(Vec::new());
    };
    metadata::array(jars, DOCUMENT, "quilt_loader.jars")?
        .iter()
        .map(|jar| {
            jar.as_str()
                .map(str::to_owned)
                .ok_or_else(|| metadata::invalid(DOCUMENT, "quilt_loader.jars[]", "a string"))
        })
        .collect()
}

fn parse(contents: &str) -> Result<ModInfo> {
    let document = "quilt.mod.json";
    let data: Value = serde_json::from_str(contents)?;
    metadata::object(&data, document, "root")?;
    let loader = data
        .get("quilt_loader")
        .ok_or_else(|| anyhow::anyhow!("{document}: missing quilt_loader field"))?;
    metadata::object(loader, document, "quilt_loader")?;
    let modid = metadata::required_string(loader, "id", document)?.to_owned();
    let version = metadata::required_string(loader, "version", document)?.to_owned();
    let name = match loader.get("metadata") {
        Some(display) => {
            metadata::object(display, document, "quilt_loader.metadata")?;
            metadata::optional_string(display, "name", document)?
                .unwrap_or(&modid)
                .to_owned()
        }
        None => modid.clone(),
    };
    let mut dependencies = Vec::new();
    if let Some(clauses) = loader.get("depends") {
        for clause in metadata::array(clauses, document, "quilt_loader.depends")? {
            dependencies.push(dependency(clause, document)?);
        }
    }
    Ok(ModInfo {
        nested_path: Vec::new(),
        name,
        modid,
        version,
        dependencies,
    })
}

fn dependency(clause: &Value, document: &str) -> Result<DependencyConstraint> {
    match clause {
        Value::String(target) => Ok(DependencyConstraint::Mod(ModDependency {
            modid: target.clone(),
            version_requirement: VersionConstraint::Any,
        })),
        Value::Array(clauses) => Ok(DependencyConstraint::AnyOf(
            clauses
                .iter()
                .map(|clause| dependency(clause, document))
                .collect::<Result<_>>()?,
        )),
        Value::Object(_) => {
            let modid = metadata::required_string(clause, "id", document)?.to_owned();
            let constraint = clause.get("versions").or_else(|| clause.get("version"));
            let version_requirement = constraint
                .map(constraints::quilt_json)
                .transpose()
                .with_context(|| format!("{document}: invalid dependency constraint for {modid}"))?
                .unwrap_or(VersionConstraint::Any);
            Ok(DependencyConstraint::Mod(ModDependency {
                modid,
                version_requirement,
            }))
        }
        _ => Err(metadata::invalid(
            document,
            "depends",
            "a mod ID, dependency object, or dependency group",
        )),
    }
}
