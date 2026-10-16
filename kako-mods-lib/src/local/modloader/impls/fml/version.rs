//! FML version substitution using document properties and shared JAR Manifest parsing.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};

use crate::local::java::jar::{DuplicateHeaders, attributes};

pub(super) fn resolve(
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
        if variable == "file.jarVersion" {
            if manifest_version.is_none() {
                let contents =
                    read_manifest()?.context("missing META-INF/MANIFEST.MF for file.jarVersion")?;
                let attributes = attributes(&contents, DuplicateHeaders::LastWins)?;
                manifest_version = Some(
                    attributes
                        .get("implementation-version")
                        .context("missing Implementation-Version for file.jarVersion")?
                        .clone(),
                );
            }
            return Ok(manifest_version.as_ref().unwrap().clone());
        }
        let key = variable
            .strip_prefix("file.")
            .with_context(|| format!("unsupported version variable {variable}"))?;
        let value = document
            .get("properties")
            .and_then(|properties| properties.get(key))
            .with_context(|| format!("missing version variable {variable}"))?;
        match value {
            toml::Value::String(value) => Ok(value.clone()),
            toml::Value::Integer(_) | toml::Value::Float(_) | toml::Value::Boolean(_) => {
                Ok(value.to_string())
            }
            _ => bail!("version variable {variable} must contain a scalar value"),
        }
    })
}

fn expand(
    expression: &str,
    visiting: &mut HashSet<String>,
    lookup: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let mut output = String::new();
    let mut remaining = expression;
    while let Some(start) = remaining.find("${") {
        output.push_str(&remaining[..start]);
        let reference = &remaining[start + 2..];
        let end = reference
            .find('}')
            .context("unclosed version variable reference")?;
        let variable = &reference[..end];
        if visiting.len() >= 128 {
            bail!("version variable nesting exceeds 128 levels");
        }
        if !visiting.insert(variable.to_owned()) {
            bail!("cyclic version variable reference {variable}");
        }
        let value = lookup(variable)?;
        output.push_str(&expand(&value, visiting, lookup)?);
        visiting.remove(variable);
        remaining = &reference[end + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}
