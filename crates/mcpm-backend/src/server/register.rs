//! `server.register`: writes a complete registration and optional local configuration.
//!
//! Metadata is supplied in full; registration never rereads a script or requires a
//! recognition token. The workspace writer lock covers Git setup and configuration
//! publication, so concurrent commands cannot observe a half-written workspace.

use std::{fs, future::Future, io, path::PathBuf, time::Instant};

use anyhow::{Context as _, ensure};
use kako_mc_lib::local::modloader::BUILTIN_LOADERS;
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::data::server::config::{self, OverwritePolicy};
use crate::data::server::{
    MinecraftServer, MinecraftServerError, MinecraftServerMetadata,
};
use crate::data::workspace::manifest::{Kind, WorkspaceError};
use crate::data::workspace::{self, RegistrationWriter};
use crate::error::OperationError;
use crate::workspace::git;

use super::types::{Initialized, Initialize, Phase, ServerInfo};

/// Registers a Local Server on the host owning it.
#[derive(Debug)]
pub struct Register;

crate::host_operation!(Register, "server.register");

impl Operation for Register {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = Initialize;
    type Event = RegisterEvent;
    type Output = Initialized;
    type Error = OperationError<RegisterKind>;

    fn run(
        input: Initialize,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || register(input, &emitter))
                .await
                .map_err(|error: RegisterFailure| error.into_operation())
        }
    }
}

/// One registration phase.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegisterEvent {
    Stage { phase: Phase },
}

fn register(
    args: Initialize,
    emitter: &mcpm_exec::Emitter<RegisterEvent>,
) -> Result<Initialized, RegisterFailure> {
    let started = Instant::now();
    let stage = |phase| {
        emitter
            .emit(RegisterEvent::Stage { phase })
            .map_err(RegisterFailure::Cancelled)
    };
    stage(Phase::Validating)?;
    git::validate_options(args.git, args.no_git).map_err(RegisterFailure::Git)?;
    tracing::info!(path = %args.path.display(), force = args.force, "Validating complete server initialization data");
    require_local(&args.path)?;
    validate_metadata(&args.metadata).map_err(RegisterFailure::Input)?;
    let manifest = args.path.join(".mcpm/server.json");
    match fs::symlink_metadata(&manifest) {
        Ok(metadata) => {
            if !args.force {
                return Err(RegisterFailure::Input(anyhow::Error::new(
                    InitializationError::AlreadyExists { path: manifest },
                )));
            }
            if metadata.is_dir() {
                return Err(RegisterFailure::Input(anyhow::Error::new(
                    InitializationError::ManifestDirectory { path: manifest },
                )));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(RegisterFailure::Registration(
                anyhow::Error::new(error)
                    .context("failed to inspect existing server configuration"),
            ));
        }
    }
    // This is the final cancellation boundary before the atomic write. A later
    // cancellation or disconnected client cannot roll back a completed registration.
    emitter
        .checkpoint()
        .map_err(RegisterFailure::Cancelled)?;
    stage(Phase::Writing)?;
    emitter
        .checkpoint()
        .map_err(RegisterFailure::Cancelled)?;
    tracing::info!(path = %args.path.display(), force = args.force, "Writing server registration metadata");
    let writer = RegistrationWriter::acquire(&args.path).map_err(RegisterFailure::Registration)?;
    require_local(&args.path)?;
    let git = if args.no_git {
        None
    } else {
        Some(git::ensure_repository(&args.path, args.git).map_err(RegisterFailure::Git)?)
    };
    let policy = OverwritePolicy {
        replace_registration: args.force,
        replace_conf: args.replace_conf,
    };
    let publication = Some(
        config::publish(&args.path, &args.metadata, args.configuration.as_ref(), policy)
            .map_err(RegisterFailure::Configuration)?,
    );
    let server = if let Some(publication) = &publication {
        publication.server.clone()
    } else if args.force {
        MinecraftServer::replace(args.path, args.metadata).map_err(RegisterFailure::Server)?
    } else {
        MinecraftServer::create(args.path, args.metadata).map_err(RegisterFailure::Server)?
    };
    writer
        .mark_local()
        .context("Local server registration/configuration was written, but publishing mcpm.json failed; inspect the completed files before retrying")
        .map_err(RegisterFailure::Registration)?;
    tracing::info!(path = %server.path().display(), "Server registration written");
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        "Server initialization completed"
    );
    let mut info = ServerInfo::from_server(&server);
    if publication.is_some() {
        info.configuration =
            Some(config::configuration_files(server.path()).map_err(RegisterFailure::Configuration)?);
    }
    Ok(Initialized {
        server: info,
        git,
        publication: publication.map(|value| value.summary()),
    })
}

fn require_local(path: &std::path::Path) -> Result<(), RegisterFailure> {
    match workspace::kind(path) {
        Ok(Kind::Local) => Ok(()),
        Ok(Kind::Remote) => Err(RegisterFailure::Registration(anyhow::Error::new(
            WorkspaceError::RemoteTargetNotLocal { path: path.to_owned() },
        ))),
        Err(error) => Err(RegisterFailure::Registration(error)),
    }
}

fn validate_metadata(metadata: &MinecraftServerMetadata) -> anyhow::Result<()> {
    if let Some(loader) = &metadata.loader {
        ensure!(
            BUILTIN_LOADERS.contains_key(loader.loader_type.as_str()),
            "unsupported server loader {:?}",
            loader.loader_type
        );
    }
    if let Some(start) = &metadata.start {
        ensure!(
            !start.executable.trim().is_empty()
                && !start.executable.contains('\0')
                && start.arguments.iter().all(|arg| !arg.contains('\0')),
            "invalid server start snapshot: executable must be nonempty, and executable/arguments must not contain NUL"
        );
    }
    Ok(())
}

#[derive(Debug, Error)]
enum InitializationError {
    #[error("server configuration already exists at {path:?}; use --force to replace it")]
    AlreadyExists { path: PathBuf },
    #[error("server configuration path is a directory: {path:?}")]
    ManifestDirectory { path: PathBuf },
}

#[derive(Debug, Error)]
enum RegisterFailure {
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(transparent)]
    Registration(anyhow::Error),
    #[error("Git repository setup failed")]
    Git(#[source] anyhow::Error),
    #[error("server configuration publication failed")]
    Configuration(#[source] anyhow::Error),
    #[error(transparent)]
    Server(MinecraftServerError),
    #[error("server initialization was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
}

impl RegisterFailure {
    fn into_operation(self) -> OperationError<RegisterKind> {
        let kind = match &self {
            Self::Input(error) => match error.downcast_ref::<InitializationError>() {
                Some(InitializationError::AlreadyExists { .. }) => RegisterKind::AlreadyExists,
                Some(InitializationError::ManifestDirectory { .. }) => {
                    RegisterKind::ManifestDirectory
                }
                None => RegisterKind::InvalidMetadata,
            },
            Self::Registration(error) => {
                if error.downcast_ref::<WorkspaceError>().is_some() {
                    RegisterKind::RemoteWorkspace
                } else {
                    RegisterKind::Registration
                }
            }
            Self::Git(_) => RegisterKind::Git,
            Self::Configuration(_) => RegisterKind::Configuration,
            Self::Server(_) => RegisterKind::Registration,
            Self::Cancelled(_) => RegisterKind::Cancelled,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a registration failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterKind {
    AlreadyExists,
    ManifestDirectory,
    InvalidMetadata,
    RemoteWorkspace,
    Registration,
    Git,
    Configuration,
    Cancelled,
}
