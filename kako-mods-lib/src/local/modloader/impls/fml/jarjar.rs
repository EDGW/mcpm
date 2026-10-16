//! Forge/NeoForge Jar-in-Jar declarations, as defined by the official JarJar metadata serializer.

use anyhow::Result;
use serde_json::Value;

use super::super::utils::metadata;

pub(super) const DOCUMENT: &str = "META-INF/jarjar/metadata.json";

pub(super) fn paths(contents: &str) -> Result<Vec<String>> {
    let data: Value = serde_json::from_str(contents)?;
    metadata::object(&data, DOCUMENT, "root")?;
    let jars = data
        .get("jars")
        .ok_or_else(|| metadata::invalid(DOCUMENT, "jars", "an array"))?;
    metadata::array(jars, DOCUMENT, "jars")?
        .iter()
        .map(|jar| {
            metadata::object(jar, DOCUMENT, "jars[]")?;
            for (field, keys) in [
                ("identifier", ["group", "artifact"]),
                ("version", ["range", "artifactVersion"]),
            ] {
                let value = jar
                    .get(field)
                    .ok_or_else(|| metadata::invalid(DOCUMENT, field, "an object"))?;
                metadata::object(value, DOCUMENT, field)?;
                for key in keys {
                    metadata::required_string(value, key, DOCUMENT)?;
                }
            }
            if jar
                .get("isObfuscated")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(metadata::invalid(DOCUMENT, "isObfuscated", "a boolean"));
            }
            Ok(metadata::required_string(jar, "path", DOCUMENT)?.to_owned())
        })
        .collect()
}
