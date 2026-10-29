//! Provider construction and selection.
//!
//! Only surface operations reach this module: it interprets user specifications, including
//! CurseForge API keys, and builds configured providers over the caller's HTTP client.

use anyhow::{Result, bail};
use kako_mc_lib::source::{
    Source, SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource,
};

/// Sources used when the caller does not select any explicitly.
pub const DEFAULT_SPECS: [&str; 2] = ["modrinth", "curseforge"];

/// Builds one provider from a source specification.
///
/// Accepted forms are `modrinth`, `curseforge` (Curse.tools) and `curseforge@KEY`
/// (official API). The key stays inside this process.
pub fn build(client: &reqwest::Client, specification: &str) -> Result<Box<dyn Source>> {
    match specification {
        "modrinth" => Ok(Box::new(ModrinthSource::with_endpoint(
            SourceId("modrinth".into()),
            client.clone(),
            "https://api.modrinth.com/v2/".parse()?,
        )?)),
        "curseforge" => Ok(Box::new(CurseForgeSource::with_endpoint(
            SourceId("curseforge".into()),
            client.clone(),
            "https://api.curse.tools/v1/cf/".parse()?,
            None,
        )?)),
        value if value.starts_with("curseforge@") => {
            let key = &value["curseforge@".len()..];
            if key.is_empty() {
                bail!("curseforge@ requires a non-empty API key");
            }
            Ok(Box::new(CurseForgeSource::with_endpoint(
                SourceId("curseforge".into()),
                client.clone(),
                "https://api.curseforge.com/v1/".parse()?,
                Some(key),
            )?))
        }
        _ => bail!("source must be modrinth, curseforge, or curseforge@KEY"),
    }
}

/// Resolves the ordered specifications a command should build.
///
/// Explicit selections replace the default pair; no selection means both defaults.
pub fn select(sources: Option<Vec<String>>, explicit: Vec<String>) -> Result<Vec<String>> {
    let mut selected = sources.unwrap_or_else(|| {
        if explicit.is_empty() {
            DEFAULT_SPECS.iter().map(|spec| (*spec).to_owned()).collect()
        } else {
            Vec::new()
        }
    });
    selected.extend(explicit);
    anyhow::ensure!(!selected.is_empty(), "select at least one source");
    anyhow::ensure!(
        selected.iter().all(|source| !source.is_empty()),
        "source names cannot be empty"
    );
    Ok(selected)
}

/// The provider family a specification names, without any credential.
pub fn family(specification: &str) -> &str {
    specification
        .split('@')
        .next()
        .unwrap_or(specification)
}
