use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result, bail};

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
                let attributes = manifest_attributes(&contents)?;
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

fn manifest_attributes(contents: &str) -> Result<BTreeMap<String, String>> {
    let mut attributes = BTreeMap::<String, String>::new();
    let mut previous: Option<String> = None;
    for line in contents.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(continuation) = line.strip_prefix(' ') {
            let key = previous
                .as_ref()
                .context("manifest continuation has no header")?;
            attributes
                .get_mut(key)
                .context("manifest continuation header is missing")?
                .push_str(continuation);
        } else {
            let (name, value) = line.split_once(": ").context("invalid manifest header")?;
            let name = name.to_ascii_lowercase();
            attributes.insert(name.clone(), value.to_owned());
            previous = Some(name);
        }
    }
    Ok(attributes)
}
