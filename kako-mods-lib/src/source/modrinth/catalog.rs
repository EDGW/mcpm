//! Retrieve advisory Minecraft version identifiers from Modrinth's tag catalog.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::ModrinthSource;

impl ModrinthSource {
    pub(super) async fn fetch_game_versions(&self) -> Result<Vec<String>> {
        let versions: Vec<GameVersion> = self
            .request("tag/game_version")?
            .send()
            .await
            .context("Modrinth game version request failed")?
            .error_for_status()
            .context("Modrinth game version HTTP error")?
            .json()
            .await
            .context("invalid Modrinth game version response")?;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for item in versions {
            ensure!(
                !item.version.trim().is_empty(),
                "empty Modrinth game version identifier"
            );
            if seen.insert(item.version.clone()) {
                result.push(item.version);
            }
        }
        Ok(result)
    }
}

#[derive(Deserialize, Debug)]
struct GameVersion {
    version: String,
}
