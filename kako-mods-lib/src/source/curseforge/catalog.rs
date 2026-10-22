//! Retrieve advisory Minecraft version identifiers from CurseForge's game catalog.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{CurseForgeSource, api};

impl CurseForgeSource {
    pub(super) async fn fetch_game_versions(&self) -> Result<Vec<String>> {
        let response: api::Response<Vec<GameVersion>> = self
            .request("minecraft/version")?
            .send()
            .await
            .context("CurseForge game version request failed")?
            .error_for_status()
            .context("CurseForge game version HTTP error")?
            .json()
            .await
            .context("invalid CurseForge game version response")?;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for item in response.data {
            ensure!(
                !item.version_string.trim().is_empty(),
                "empty CurseForge game version identifier"
            );
            if seen.insert(item.version_string.clone()) {
                result.push(item.version_string);
            }
        }
        Ok(result)
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GameVersion {
    version_string: String,
}
