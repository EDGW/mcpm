//! Forge/NeoForge Jar-in-Jar declarations, as defined by the official JarJar metadata serializer.

use anyhow::Result;
use serde_json::Value;

use super::super::utils::metadata;

pub(super) const DOCUMENT: &str = "META-INF/jarjar/metadata.json";

pub(super) fn paths(contents: &str) -> Result<Vec<String>> {
    let data: Value = serde_json::from_str(contents)?;
    metadata::object(&data, DOCUMENT, "root")?;
    let jars = metadata::required(&data, "jars", DOCUMENT)?;
    metadata::array(jars, DOCUMENT, "jars")?
        .iter()
        .map(|jar| {
            metadata::object(jar, DOCUMENT, "jars[]")?;
            for (field, keys) in [
                ("identifier", ["group", "artifact"]),
                ("version", ["range", "artifactVersion"]),
            ] {
                let value = metadata::required(jar, field, DOCUMENT)?;
                metadata::object(value, DOCUMENT, field)?;
                for key in keys {
                    metadata::required_string(value, key, DOCUMENT)?;
                }
            }
            if jar
                .get("isObfuscated")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(metadata::invalid(DOCUMENT, "isObfuscated", "a boolean").into());
            }
            Ok(metadata::required_string(jar, "path", DOCUMENT)?.to_owned())
        })
        .collect()
}
