use anyhow::{Context, Result};
use serde_json::Value;

use crate::local::mcmod::{DependencyConstraint, ModDependency, ModInfo};

use super::{constraints, metadata};

pub(super) fn requires_platform(contents: &str, document: &str, platform: &str) -> Result<bool> {
    let native: toml::Value =
        toml::from_str(contents).with_context(|| format!("failed to parse {document}"))?;
    let data = serde_json::to_value(native)?;
    let Some(declarations) = data.get("mods") else {
        return Ok(false);
    };
    for declaration in metadata::array(declarations, document, "mods")? {
        let modid = metadata::required_string(declaration, "modId", document)?;
        if let Some(clauses) = dependency_clauses(&data, modid, document)? {
            for clause in clauses {
                if clause.get("modId").and_then(serde_json::Value::as_str) == Some(platform)
                    && (clause.get("mandatory").and_then(serde_json::Value::as_bool) == Some(true)
                        || clause.get("type").and_then(serde_json::Value::as_str)
                            == Some("required"))
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub(super) fn parse(contents: &str, document: &str) -> Result<Vec<ModInfo>> {
    let native: toml::Value =
        toml::from_str(contents).with_context(|| format!("failed to parse {document}"))?;
    let data = serde_json::to_value(native)?;
    let declarations = data
        .get("mods")
        .ok_or_else(|| anyhow::anyhow!("{document}: missing mods field"))?;
    let declarations = metadata::array(declarations, document, "mods")?;
    if declarations.is_empty() {
        return Err(metadata::invalid(
            document,
            "mods",
            "at least one mod declaration",
        ));
    }
    let mut mods = Vec::new();
    for declaration in declarations {
        let modid = metadata::required_string(declaration, "modId", document)?.to_owned();
        let name = metadata::optional_string(declaration, "displayName", document)?
            .unwrap_or(&modid)
            .to_owned();
        let version = metadata::optional_string(declaration, "version", document)?
            .unwrap_or("1")
            .to_owned();
        let mut dependencies = Vec::new();
        if let Some(clauses) = dependency_clauses(&data, &modid, document)? {
            for clause in clauses {
                let target = metadata::required_string(clause, "modId", document)?;
                let requirement =
                    metadata::optional_string(clause, "versionRange", document)?.unwrap_or("");
                dependencies.push(DependencyConstraint::Mod(ModDependency {
                    modid: target.into(),
                    version_requirement: constraints::maven(requirement).with_context(|| {
                        format!("{document}: invalid dependency range for {target}")
                    })?,
                }));
            }
        }
        mods.push(ModInfo {
            nested_path: Vec::new(),
            name,
            modid,
            version,
            dependencies,
        });
    }
    Ok(mods)
}

// Forge requests dependencies.<modid>, rather than validating every top-level
// dependency entry. An unscoped [[dependencies]] list has no matching path.
fn dependency_clauses<'a>(
    data: &'a Value,
    modid: &str,
    document: &str,
) -> Result<Option<&'a Vec<Value>>> {
    data.get("dependencies")
        .and_then(|dependencies| dependencies.get(modid))
        .map(|clauses| metadata::array(clauses, document, &format!("dependencies.{modid}")))
        .transpose()
}
