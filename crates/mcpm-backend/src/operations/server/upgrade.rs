//! Host-side registration upgrades; user decisions stay on the calling frontend.

use std::path::PathBuf;

use anyhow::Result;
use mcpm_exec::operation::Operation;
use mcpm_lib::local::mc::server::{config, upgrade};
use serde::{Deserialize, Serialize};

use super::info::ServerInfo;

pub use upgrade::{UpgradeDecision, UpgradePreview, VanillaCandidate};

pub struct PrepareUpgrade;
impl Operation for PrepareUpgrade {
    const ID: &'static str = "server.metadata.upgrade.prepare";
    type Input = PrepareUpgradeInput;
    type Output = UpgradePreview;

    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            crate::workspace::require_local(&input.path)?;
            upgrade::prepare(&input.path)
        })
        .await?
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareUpgradeInput {
    pub path: PathBuf,
}

pub struct ApplyUpgrade;
impl Operation for ApplyUpgrade {
    const ID: &'static str = "server.metadata.upgrade.apply";
    type Input = ApplyUpgradeInput;
    type Output = ServerInfo;

    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            crate::workspace::require_local(&input.path)?;
            let server = upgrade::apply(&input.path, &input.expected_revision, input.decision)?;
            let mut info = ServerInfo::from_server(&server);
            info.configuration = Some(config::configuration_files(server.path())?);
            Ok(info)
        })
        .await?
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyUpgradeInput {
    pub path: PathBuf,
    pub expected_revision: String,
    pub decision: UpgradeDecision,
}
