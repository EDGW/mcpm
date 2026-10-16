//! FML version substitution using file properties and JAR Manifest attributes.
//!
//! Forge and NeoForge default absent Implementation-Version to `0.0NONE`.
//! Their StringSubstitutor uses Apache Commons Lang defaults: recursive values,
//! `$` escaping and unchanged unterminated references. Forge runtime globals
//! require a running loader and cannot be inferred from a standalone mod;
//! NeoForge treats that namespace as ordinary literal text.
//!
//! Sources:
//! - <https://github.com/MinecraftForge/MinecraftForge/blob/1.20.x/fmlloader/src/main/java/net/minecraftforge/fml/loading/moddiscovery/ModFile.java>
//! - <https://github.com/neoforged/FancyModLoader/blob/main/loader/src/main/java/net/neoforged/fml/loading/moddiscovery/ModFile.java>
//! - <https://github.com/neoforged/FancyModLoader/blob/main/loader/src/main/java/net/neoforged/fml/loading/StringSubstitutor.java>

use std::collections::HashSet;

use anyhow::{Result, bail};

use crate::local::java::jar::{DuplicateHeaders, attributes};

/// Loader-specific substitution namespaces; file properties share one implementation.
#[derive(Clone, Copy)]
pub(super) enum VersionFormat {
    Forge,
    NeoForge,
}

pub(super) fn resolve(
    format: VersionFormat,
    expression: &str,
    document: &str,
    mut read_manifest: impl FnMut() -> Result<Option<String>>,
) -> Result<String> {
    if !expression.contains("${") {
        return Ok(expression.to_owned());
    }
    let document: toml::Value = toml::from_str(document)?;
    let mut manifest_version: Option<String> = None;
    let mut visiting = HashSet::new();
    expand(expression, &mut visiting, &mut |variable| {
        // Both loaders split on dots and use the first file-property segment.
        let mut parts = variable.trim_end_matches('.').split('.');
        let prefix = parts.next().unwrap_or_default();
        let Some(key) = parts.next() else {
            return Ok(Some(variable.to_owned()));
        };
        if prefix == "global" && matches!(format, VersionFormat::Forge) {
            return match key {
                "mcVersion" | "forgeVersion" => {
                    bail!(
                        "runtime version variable {variable} is unavailable during local inspection"
                    )
                }
                _ => Ok(None),
            };
        }
        if prefix != "file" {
            return Ok(Some(variable.to_owned()));
        }
        if key == "jarVersion" {
            if manifest_version.is_none() {
                let contents = read_manifest()?.unwrap_or_default();
                let attributes = attributes(&contents, DuplicateHeaders::LastWins)?;
                manifest_version = Some(
                    attributes
                        .get("implementation-version")
                        .cloned()
                        .unwrap_or_else(|| "0.0NONE".to_owned()),
                );
            }
            return Ok(manifest_version.clone());
        }
        // String.valueOf(null) in FML yields "null", not an unresolved reference.
        Ok(Some(
            document
                .get("properties")
                .and_then(|properties| properties.get(key))
                .map(property_string)
                .unwrap_or_else(|| "null".to_owned()),
        ))
    })
}

fn expand(
    expression: &str,
    visiting: &mut HashSet<String>,
    lookup: &mut impl FnMut(&str) -> Result<Option<String>>,
) -> Result<String> {
    let mut output = String::new();
    let mut remaining = expression;
    while let Some(start) = remaining.find("${") {
        output.push_str(&remaining[..start]);
        if output.ends_with('$') {
            output.pop();
            output.push_str("${");
            remaining = &remaining[start + 2..];
            continue;
        }
        let reference = &remaining[start + 2..];
        let Some(end) = reference.find('}') else {
            output.push_str(&remaining[start..]);
            return Ok(output);
        };
        let expression = &reference[..end];
        let (variable, default) = expression
            .find(":-")
            .filter(|index| !expression[..*index].contains("${"))
            .map_or((expression, None), |index| {
                (&expression[..index], Some(&expression[index + 2..]))
            });
        if visiting.len() >= 128 {
            bail!("version variable nesting exceeds 128 levels");
        }
        if !visiting.insert(variable.to_owned()) {
            bail!("cyclic version variable reference {variable}");
        }
        let value = lookup(variable)?.or_else(|| default.map(str::to_owned));
        if let Some(value) = value {
            output.push_str(&expand(&value, visiting, lookup)?);
        } else {
            output.push_str(&remaining[start..start + 2 + end + 1]);
        }
        visiting.remove(variable);
        remaining = &reference[end + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}

/// Matches Java's String.valueOf for TOML-backed file properties.
fn property_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        toml::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(property_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        toml::Value::Table(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{key}={}", property_string(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}
