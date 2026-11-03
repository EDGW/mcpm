//! Host-only validation and Git preparation before a local Remote stub is registered.

use super::{super::info::ServerInfo, Initialized, git};
use anyhow::Result;
use mcpm_exec::operation::Operation;
use mcpm_lib::local::mc::server::config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct PrepareRemote;
impl Operation for PrepareRemote {
    const ID: &'static str = "server.init.prepare_remote";
    type Input = PrepareRemoteInput;
    type Output = Initialized;
    async fn run(input: Self::Input) -> Result<Initialized> {
        tokio::task::spawn_blocking(move || {
            git::validate_options(input.git, input.no_git)?;
            let server = crate::workspace::inspect_local(input.path)?;
            let mut info = ServerInfo::from_server(&server);
            info.configuration = Some(config::configuration_files(server.path())?);
            let git = if input.no_git {
                None
            } else {
                Some(git::ensure_repository(server.path(), input.git)?)
            };
            Ok(Initialized {
                server: info,
                git,
                publication: None,
            })
        })
        .await?
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareRemoteInput {
    pub path: PathBuf,
    pub no_git: bool,
    pub git: bool,
}
