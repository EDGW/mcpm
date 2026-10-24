//! Read-only server recognition and independent complete-data initialization on the file-owning host.

use std::{fs, io, path::PathBuf, time::Instant};

use anyhow::{Context, Result, bail, ensure};
use mcpm_backend::local::{
    mc::server::{MinecraftServer, MinecraftServerMetadata, identify},
    modloader::BUILTIN_LOADERS,
};
use thiserror::Error;

use super::super::workspace::{self, Writer};
use super::model::{Candidate, Initialize, Phase, Recognize};

/// Recognition never loads registration metadata, creates directories, or writes configuration.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn recognize(
    args: Recognize,
    checkpoint: impl Fn() -> Result<()>,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Vec<Candidate>> {
    checkpoint()?;
    validate_options(&args)?;
    phase(Phase::Recognizing)?;
    checkpoint()?;
    let started = Instant::now();
    let script = args
        .start_script
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "run.bat" } else { "run.sh" }));
    tracing::info!(path = %args.path.display(), script = %script.display(), "Recognizing server launch candidates");
    let mut candidates = identify::identify(&args.path, &script, &BUILTIN_LOADERS)
        .context("automatic server identification failed; server remains unrecognized; initialize manually with --minecraft-version VERSION --loader forge|neoforge|fabric --loader-version VERSION, or explicitly use --loader vanilla for a known vanilla server")?;
    checkpoint()?;
    candidates.retain(|candidate| {
        let metadata = &candidate.metadata;
        args.minecraft_version
            .as_ref()
            .is_none_or(|value| *value == metadata.minecraft_version)
            && args
                .loader
                .as_ref()
                .is_none_or(|loader| match &metadata.loader {
                    Some(info) => info.loader_type == *loader,
                    None => loader == "vanilla",
                })
            && args.loader_version.as_ref().is_none_or(|version| {
                metadata
                    .loader
                    .as_ref()
                    .is_some_and(|info| info.version == *version)
            })
    });
    if candidates.is_empty() {
        bail!(
            "no identified server launch matches the supplied version/loader options; server remains unrecognized; initialize manually with complete --minecraft-version, --loader and --loader-version options (omit --loader-version only for explicitly selected vanilla)"
        );
    }
    tracing::debug!(
        candidates = candidates.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "Server recognition completed without writing configuration"
    );
    Ok(candidates.into_iter().map(Candidate::from).collect())
}

/// Metadata is supplied in full; initialization never rereads a script or requires a recognition token.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn initialize(
    args: Initialize,
    checkpoint: impl Fn() -> Result<()>,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<MinecraftServer> {
    let started = Instant::now();
    checkpoint()?;
    phase(Phase::Validating)?;
    tracing::info!(path = %args.path.display(), force = args.force, "Validating complete server initialization data");
    workspace::require_local(&args.path)?;
    validate_metadata(&args.metadata)?;
    let manifest = args.path.join(".mcpm/server.json");
    match fs::symlink_metadata(&manifest) {
        Ok(metadata) => {
            ensure!(
                args.force,
                InitializationError::AlreadyExists { path: manifest }
            );
            ensure!(
                !metadata.is_dir(),
                InitializationError::ManifestDirectory { path: manifest }
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect existing server configuration"),
    }
    // This is the final cancellation boundary before the atomic write. A later
    // cancellation or disconnected client cannot roll back a completed registration.
    checkpoint()?;
    phase(Phase::Writing)?;
    checkpoint()?;
    tracing::info!(path = %args.path.display(), force = args.force, "Writing server registration metadata");
    let writer = Writer::acquire(&args.path)?;
    workspace::require_local(&args.path)?;
    let server = if args.force {
        MinecraftServer::replace(args.path, args.metadata)?
    } else {
        MinecraftServer::create(args.path, args.metadata)?
    };
    writer.mark_local().context("Local server.json was written, but publishing mcpm.json failed; inspect the workspace before retrying")?;
    tracing::info!(path = %server.path().display(), "Server registration written");
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        "Server initialization completed"
    );
    Ok(server)
}

pub(super) fn validate_options(args: &Recognize) -> Result<()> {
    for version in [&args.minecraft_version, &args.loader_version]
        .into_iter()
        .flatten()
    {
        ensure!(
            !version.trim().is_empty(),
            InitializationError::EmptyVersion
        );
    }
    if let Some(loader) = &args.loader {
        ensure!(
            loader == "vanilla" || BUILTIN_LOADERS.contains_key(loader.as_str()),
            InitializationError::UnsupportedLoader {
                loader: loader.clone()
            }
        );
    }
    ensure!(
        !(args.loader.as_deref() == Some("vanilla") && args.loader_version.is_some()),
        InitializationError::VanillaLoaderVersion
    );
    Ok(())
}

fn validate_metadata(metadata: &MinecraftServerMetadata) -> Result<()> {
    if let Some(loader) = &metadata.loader {
        ensure!(
            BUILTIN_LOADERS.contains_key(loader.loader_type.as_str()),
            InitializationError::UnsupportedLoader {
                loader: loader.loader_type.clone()
            }
        );
    }
    if let Some(start) = &metadata.start {
        ensure!(
            !start.script.as_os_str().is_empty()
                && !start.directory.as_os_str().is_empty()
                && !start.executable.trim().is_empty()
                && !start.executable.contains('\0')
                && start.arguments.iter().all(|arg| !arg.contains('\0')),
            InitializationError::InvalidStart
        );
    }
    // Metadata format and nonempty versions are validated by MinecraftServer's
    // persistence API for both public caller-constructed and deserialized inputs.
    Ok(())
}

#[derive(Debug, Error)]
enum InitializationError {
    #[error("server configuration already exists at {path:?}; use --force to replace it")]
    AlreadyExists { path: PathBuf },
    #[error("server configuration path is a directory: {path:?}")]
    ManifestDirectory { path: PathBuf },
    #[error("version options must not be empty")]
    EmptyVersion,
    #[error("unsupported server loader {loader:?}")]
    UnsupportedLoader { loader: String },
    #[error("vanilla does not have a loader version")]
    VanillaLoaderVersion,
    #[error(
        "invalid server start snapshot: script, directory and executable must be nonempty, and executable/arguments must not contain NUL"
    )]
    InvalidStart,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug, Error)]
    #[error("initialization operation cancelled")]
    struct Cancelled;
    #[derive(Debug, Error)]
    #[error("initialization event consumer failed")]
    struct ConsumerFailed;

    fn args(path: &std::path::Path, force: bool) -> Initialize {
        Initialize {
            path: path.into(),
            metadata: serde_json::from_value(serde_json::json!({
                "version":1,"minecraft_version":"1.21.1","loader":null
            }))
            .unwrap(),
            force,
        }
    }

    #[test]
    fn cancellation_or_consumer_failure_at_each_prewrite_phase_preserves_metadata() {
        for stop in [Phase::Validating, Phase::Writing] {
            for force in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let manifest = root.path().join(".mcpm/server.json");
                let old = b"existing invalid configuration\n";
                if force {
                    fs::create_dir(root.path().join(".mcpm")).unwrap();
                    fs::write(&manifest, old).unwrap();
                }
                let cancelled = Cell::new(false);
                let error = initialize(
                    args(root.path(), force),
                    || {
                        if cancelled.get() {
                            return Err(Cancelled.into());
                        }
                        Ok(())
                    },
                    |phase| {
                        if phase == stop {
                            cancelled.set(true);
                        }
                        Ok(())
                    },
                )
                .unwrap_err();
                assert!(error.is::<Cancelled>(), "{error:#}");
                let error = initialize(
                    args(root.path(), force),
                    || Ok(()),
                    |phase| {
                        if phase == stop {
                            return Err(ConsumerFailed.into());
                        }
                        Ok(())
                    },
                )
                .unwrap_err();
                assert!(error.is::<ConsumerFailed>(), "{error:#}");
                if force {
                    assert_eq!(fs::read(&manifest).unwrap(), old);
                } else {
                    assert!(!root.path().join(".mcpm").exists());
                }
            }
        }
    }

    #[test]
    fn registration_created_after_validation_cannot_be_clobbered() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join(".mcpm/server.json");
        let winner = b"a concurrent initialization won\n";
        let error = initialize(
            args(root.path(), false),
            || Ok(()),
            |phase| {
                if phase == Phase::Writing {
                    fs::create_dir(root.path().join(".mcpm"))?;
                    fs::write(&manifest, winner)?;
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(
            error
                .downcast_ref::<mcpm_backend::local::mc::server::MinecraftServerError>()
                .is_some()
        );
        assert_eq!(fs::read(manifest).unwrap(), winner);
    }

    #[test]
    fn recognition_cancelled_at_phase_boundary_does_not_even_read_missing_script() {
        let root = tempfile::tempdir().unwrap();
        let cancelled = Cell::new(false);
        let recognition: Recognize =
            serde_json::from_value(serde_json::json!({"path":root.path()})).unwrap();
        let error = recognize(
            recognition,
            || {
                if cancelled.get() {
                    return Err(Cancelled.into());
                }
                Ok(())
            },
            |_| {
                cancelled.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.is::<Cancelled>(), "{error:#}");
        assert!(!root.path().join(".mcpm").exists());
    }
}
