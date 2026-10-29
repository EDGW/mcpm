//! Fabric mod metadata, schema-specific dependencies and declared nested JARs.
//!
//! Version strings follow Fabric's VersionParser: nonempty text is preserved,
//! including non-SemVer strings; neither Manifest lookup nor FML expansion applies.
//! <https://github.com/FabricMC/fabric-loader/blob/master/src/main/java/net/fabricmc/loader/impl/util/version/VersionParser.java>

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use super::super::utils::{archive, metadata};
use super::{FabricError, constraints};
use crate::local::mcmod::{
    ModInfo,
    dependency::{Dependency, DependencyKind},
};

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

fn nested_paths(contents: &str) -> Result<Vec<String>> {
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

fn parse(contents: &str) -> Result<ModInfo> {
    let document = "fabric.mod.json";
    let data: Value = serde_json::from_str(contents)?;
    metadata::object(&data, document, "root")?;
    let modid = metadata::required_string(&data, "id", document)?.to_owned();
    // Fabric VersionParser requires a nonempty string, then accepts either
    // SemanticVersion or StringVersion. It does not expand FML placeholders.
    let version = metadata::required_string(&data, "version", document)?;
    if version.is_empty() {
        return Err(metadata::invalid(document, "version", "a non-empty string").into());
    }
    let version = version.to_owned();
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
                    id: target.clone(),
                    version_requirement: constraints::fabric_json(constraint).with_context(
                        || format!("{document}: invalid {key} constraint for {target}"),
                    )?,
                    side: None,
                    ordering: None,
                    reason: None,
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
