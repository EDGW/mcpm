//! Transport-safe snapshots of server metadata, captured on the machine owning the server.

use mcpm_backend::local::mc::server::MinecraftServer;
use serde::{Deserialize, Serialize};

mod remote;
pub(super) use remote::load as load_ssh;

/// Resolved metadata for presentation; paths retain their originating platform's spelling.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerInfo {
    pub name: String,
    pub path: String,
    pub minecraft_version: String,
    pub loader: Option<LoaderInfo>,
    pub start: Option<StartCommand>,
}

impl ServerInfo {
    pub(super) fn from_server(server: &MinecraftServer) -> Self {
        let metadata = server.metadata();
        Self {
            name: server.name().into(),
            path: server.path().display().to_string(),
            minecraft_version: metadata.minecraft_version.clone(),
            loader: server.loader().map(|info| LoaderInfo {
                id: info.loader.id().as_str().into(),
                name: info.loader.name().into(),
                version: info.version,
            }),
            start: metadata.start.as_ref().map(|start| StartCommand {
                directory: start.directory.display().to_string(),
                executable: start.executable.clone(),
                arguments: start.arguments.clone(),
                windows: start.windows,
            }),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoaderInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartCommand {
    pub directory: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}
