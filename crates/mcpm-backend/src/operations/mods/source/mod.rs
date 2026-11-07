//! Host source operations, safe configurations, and lazy configured-provider construction.

use anyhow::{Result, bail};
use configuration::Kind;
use mcpm_lib::source::{Source, SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource};
use std::time::Duration;

pub mod files;
pub mod identify;
pub mod search;
pub mod server;

mod configuration;
mod credential;

pub use configuration::{Configuration, Description};
pub use credential::Credential;

/// Constructs a request client only when the caller needs an uncached request.
pub(crate) fn build_source(configuration: &Configuration) -> Result<Box<dyn Source>> {
    let kind = configuration
        .kind()
        .ok_or_else(|| anyhow::anyhow!("source must be modrinth, curseforge, or curseforge@KEY"))?;
    if let Kind::CurseForge { key: Some(key) } = &kind
        && key.is_empty()
    {
        bail!("curseforge@ requires a non-empty API key");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    match kind {
        Kind::Modrinth => Ok(Box::new(ModrinthSource::with_endpoint(
            SourceId("modrinth".into()),
            client,
            "https://api.modrinth.com/v2/".parse()?,
        )?)),
        Kind::CurseForge { key } => Ok(Box::new(CurseForgeSource::with_endpoint(
            SourceId("curseforge".into()),
            client,
            if key.is_some() {
                "https://api.curseforge.com/v1/"
            } else {
                "https://api.curse.tools/v1/cf/"
            }
            .parse()?,
            key,
        )?)),
    }
}
