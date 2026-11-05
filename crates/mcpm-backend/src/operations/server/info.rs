//! Server metadata inspection on the file-owning host and its transport-safe snapshot.

use anyhow::Result;
use mcpm_exec::operation::Operation;
use mcpm_lib::local::mc::server::{MinecraftServer, config};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Info;
impl Operation for Info {
    const ID: &'static str = "server.info";
    type Input = InfoInput;
    type Output = ServerInfo;
    async fn run(input: InfoInput) -> Result<ServerInfo> {
        tokio::task::spawn_blocking(move || {
            let server = crate::workspace::inspect_local(input.path)?;
            let mut info = ServerInfo::from_server(&server);
            info.configuration = Some(config::configuration_files(server.path())?);
            Ok(info)
        })
        .await?
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InfoInput {
    pub path: PathBuf,
}

/// Resolved metadata; paths retain their originating platform's spelling.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerInfo {
    pub name: String,
    pub path: String,
    pub minecraft_version: String,
    /// Declaration retained even when no built-in loader supports it.
    pub declared_loader: Option<String>,
    pub loader: Option<LoaderInfo>,
    pub start: Option<StartCommand>,
    pub configuration: Option<config::ConfigurationFiles>,
}
impl ServerInfo {
    pub fn from_server(server: &MinecraftServer) -> Self {
        let metadata = server.metadata();
        Self {
            name: server.name().into(),
            path: server.path().display().to_string(),
            minecraft_version: metadata.minecraft_version.clone(),
            declared_loader: metadata
                .loader
                .as_ref()
                .map(|loader| loader.loader_type.clone()),
            loader: server.loader().map(|info| LoaderInfo {
                id: info.loader.id().as_str().into(),
                name: info.loader.name().into(),
                version: info.version,
            }),
            start: metadata.start.as_ref().map(|start| StartCommand {
                directory: server.path().display().to_string(),
                executable: start.executable.clone(),
                arguments: start.arguments.clone(),
                windows: start.windows,
            }),
            configuration: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartCommand {
    pub directory: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}
