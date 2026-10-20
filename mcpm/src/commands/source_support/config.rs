//! Shared construction of configured remote sources for CLI commands.

use std::time::Duration;

use anyhow::{Result, bail};
use kako_mods_lib::source::{
    Source, SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource,
};

pub(in crate::commands) fn build_source(source: &str) -> Result<Box<dyn Source>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    match source {
        "modrinth" => Ok(Box::new(ModrinthSource::with_endpoint(
            SourceId("modrinth".into()),
            client,
            "https://api.modrinth.com/v2/".parse()?,
        )?)),
        "curseforge" => Ok(Box::new(CurseForgeSource::with_endpoint(
            SourceId("curseforge".into()),
            client,
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
                client,
                "https://api.curseforge.com/v1/".parse()?,
                Some(key),
            )?))
        }
        _ => bail!("source must be modrinth, curseforge, or curseforge@KEY"),
    }
}
