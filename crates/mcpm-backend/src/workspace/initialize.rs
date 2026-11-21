//! Complete-data Local workspace initialization, independent of recognition and operations.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use mcpm_lib::local::{
    mc::server::{
        MinecraftServer, MinecraftServerError, MinecraftServerMetadata,
        config::{self, ConfigurationDraft, ConfigurationOverrides, OverwritePolicy, Published},
    },
    modloader::BUILTIN_LOADERS,
};

use super::{Writer, require_current_if_registered, require_local};

/// All identity and configuration decisions are supplied by the caller.
pub(crate) struct LocalInitialization {
    pub path: PathBuf,
    pub metadata: MinecraftServerMetadata,
    pub force: bool,
    pub local: Option<LocalConfiguration>,
}

/// None is an explicit registration-only request, not an instruction to infer an entry.
pub(crate) struct LocalConfiguration {
    pub configuration: Option<ConfigurationDraft>,
    pub replace_conf: bool,
    pub conf_overrides: ConfigurationOverrides,
    pub preserve_conf: bool,
}

pub(crate) enum Phase {
    Validating,
    Writing,
}

pub(crate) struct LocalInitialized<G> {
    pub server: MinecraftServer,
    pub git: G,
    pub publication: Option<Published>,
}

/// Owns the shared write workflow. Git execution is injected so this storage layer
/// does not depend on operation DTOs, while still running it under the writer lease.
pub(crate) fn initialize_local<G>(
    input: LocalInitialization,
    checkpoint: impl Fn() -> Result<()>,
    phase: impl Fn(Phase) -> Result<()>,
    prepare_git: impl FnOnce(&Path) -> Result<G>,
) -> Result<LocalInitialized<G>> {
    checkpoint()?;
    phase(Phase::Validating)?;
    require_local(&input.path)?;
    require_current_if_registered(&input.path)?;
    if let Some(loader) = input.metadata.loader.declaration() {
        ensure!(
            BUILTIN_LOADERS.contains_key(loader.loader_type.as_str()),
            "unsupported server loader {:?}",
            loader.loader_type
        );
    }
    let manifest = input.path.join(".mcpm/server.json");
    match fs::symlink_metadata(&manifest) {
        Ok(metadata) => {
            ensure!(
                input.force,
                "server configuration already exists at {manifest:?}; use --force to replace it"
            );
            ensure!(
                !metadata.is_dir(),
                "server configuration path is a directory: {manifest:?}"
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect existing server configuration"),
    }
    checkpoint()?;
    phase(Phase::Writing)?;
    checkpoint()?;
    require_current_if_registered(&input.path)?;
    let writer = Writer::acquire(&input.path)?;
    require_local(&input.path)?;
    let git = prepare_git(&input.path)?;
    let publication = input
        .local
        .map(|local| {
            config::publish(
                &input.path,
                &input.metadata,
                local.configuration.as_ref(),
                &local.conf_overrides,
                OverwritePolicy {
                    replace_registration: input.force,
                    replace_conf: local.replace_conf,
                    preserve_conf_with_overrides: local.preserve_conf,
                },
            )
        })
        .transpose()?;
    let server = if let Some(publication) = &publication {
        publication.server.clone()
    } else if input.force {
        MinecraftServer::replace(input.path, input.metadata)
            .map_err(MinecraftServerError::into_diagnostic)?
    } else {
        MinecraftServer::create(input.path, input.metadata)
            .map_err(MinecraftServerError::into_diagnostic)?
    };
    writer.mark_local().context("Local server registration/configuration was written, but publishing mcpm.json failed; inspect the completed files before retrying")?;
    Ok(LocalInitialized {
        server,
        git,
        publication,
    })
}
