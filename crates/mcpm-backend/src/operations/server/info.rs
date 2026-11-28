//! Server metadata inspection on the file-owning host and its transport-safe snapshot.

use anyhow::Result;
use mcpm_exec::{error::ErrorReport, operation::Operation};
use mcpm_lib::local::mc::server::{
    MinecraftServer, MinecraftServerError, MinecraftServerMetadata, SERVER_METADATA_VERSION,
    ServerLoader, ServerMetadataSnapshot, config, inspect_metadata, launch, properties,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct Info;
impl Operation for Info {
    const ID: &'static str = "server.info";
    type Input = InfoInput;
    type Output = ServerInfo;
    async fn run(input: InfoInput) -> Result<ServerInfo> {
        tokio::task::spawn_blocking(move || {
            crate::workspace::require_local(&input.path)?;
            let registration = inspect_metadata(input.path)?;
            let mut info = ServerInfo::from_registration(&registration)?;
            if registration.metadata_version != SERVER_METADATA_VERSION {
                let error = MinecraftServerError::UpgradeRequired {
                    path: registration.path.join(".mcpm/server.json"),
                    version: registration.metadata_version,
                }
                .into_diagnostic();
                if !input.allow_legacy {
                    return Err(error);
                }
                info.upgrade_required = Some(ErrorReport::capture(&error));
                return Ok(info);
            }
            if input.full_command {
                info.start = launch::describe(&registration.path)?.map(StartCommand::from);
            }
            Ok(info)
        })
        .await?
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InfoInput {
    pub path: PathBuf,
    #[serde(default)]
    pub full_command: bool,
    /// Only an interactive metadata viewer may request a data-only legacy snapshot.
    #[serde(default)]
    pub allow_legacy: bool,
}

/// Resolved metadata; paths retain their originating platform's spelling.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerInfo {
    pub name: String,
    pub path: String,
    pub minecraft_version: String,
    pub metadata_version: u32,
    pub loader: ServerLoaderInfo,
    pub configuration: Option<config::ConfigurationFiles>,
    pub start: Option<StartCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon: Option<RconInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_required: Option<ErrorReport>,
}
impl ServerInfo {
    /// Builds a complete current-server snapshot, including configuration and masked RCON.
    pub fn from_server(server: &MinecraftServer) -> Result<Self> {
        let mut info = Self::from_parts(
            server.path().display().to_string(),
            server.name().to_owned(),
            server.metadata_version(),
            server.metadata(),
        );
        info.read_details(server.path())?;
        Ok(info)
    }

    fn read_details(&mut self, root: &Path) -> Result<()> {
        self.configuration = Some(config::configuration_files(root)?);
        self.rcon = Some(RconInfo::read(root));
        Ok(())
    }

    fn from_registration(registration: &ServerMetadataSnapshot) -> Result<Self> {
        let mut info = Self::from_parts(
            registration.path.display().to_string(),
            registration.name.clone(),
            registration.metadata_version,
            &registration.metadata,
        );
        // Legacy inspection remains metadata-only until the explicit upgrade succeeds.
        if registration.metadata_version == SERVER_METADATA_VERSION {
            info.read_details(&registration.path)?;
        }
        Ok(info)
    }

    fn from_parts(
        path: String,
        name: String,
        metadata_version: u32,
        metadata: &MinecraftServerMetadata,
    ) -> Self {
        Self {
            path,
            name,
            minecraft_version: metadata.minecraft_version.clone(),
            metadata_version,
            loader: match &metadata.loader {
                ServerLoader::Vanilla => ServerLoaderInfo::Vanilla,
                ServerLoader::Unrecognized => ServerLoaderInfo::Unrecognized,
                ServerLoader::Modded(declaration) => {
                    let resolved = mcpm_lib::local::modloader::BUILTIN_LOADERS
                        .get(declaration.loader_type.as_str());
                    ServerLoaderInfo::Modded(LoaderInfo {
                        id: declaration.loader_type.clone(),
                        name: resolved
                            .as_ref()
                            .map(|info| info.name())
                            .unwrap_or(&declaration.loader_type)
                            .to_owned(),
                        version: declaration.version.clone(),
                    })
                }
            },
            configuration: None,
            start: None,
            rcon: None,
            upgrade_required: None,
        }
    }
}

/// Info snapshots carry only a password preview, never the underlying credentials.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RconInfo {
    Disabled,
    Enabled { port: u16, masked_password: String },
    ParseFailed,
    PropertiesParseFailed,
}

impl RconInfo {
    fn read(root: &Path) -> Self {
        let result = config::read_properties_context(root)
            .and_then(|(launch, args)| {
                properties::properties_path_from_launch(root, launch.as_ref(), &args)
            })
            .and_then(|path| properties::Properties::read(&path));
        let properties = match result {
            Ok(Some(properties)) => properties,
            Ok(None) => return Self::Disabled,
            Err(_) => return Self::PropertiesParseFailed,
        };
        match launch::rcon::Settings::from_properties(&properties) {
            Ok(Some(settings)) => Self::Enabled {
                port: settings.port(),
                masked_password: mask_password(settings.password()),
            },
            Ok(None) => Self::Disabled,
            Err(_) => Self::ParseFailed,
        }
    }
}

fn mask_password(password: &str) -> String {
    let mut chars = password.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(_)) => format!("{first}****"),
        _ => "****".into(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Identity is retained even when this build cannot resolve a concrete implementation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "state", content = "loader", rename_all = "snake_case")]
pub enum ServerLoaderInfo {
    Vanilla,
    Unrecognized,
    Modded(LoaderInfo),
}

/// Generated from current configuration, never from legacy registration snapshots.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartCommand {
    pub directory: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub windows: bool,
}

impl From<launch::CommandDescription> for StartCommand {
    fn from(command: launch::CommandDescription) -> Self {
        Self {
            directory: command.directory.display().to_string(),
            executable: command.executable,
            arguments: command.arguments,
            windows: command.windows,
        }
    }
}
