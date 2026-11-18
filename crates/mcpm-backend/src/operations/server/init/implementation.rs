//! Read-only server recognition and independent complete-data initialization on the file-owning host.

use std::{fs, path::Path, time::Instant};

use anyhow::{Context, Result};
use mcpm_lib::local::{
    mc::server::identify::{self, ServerCandidate},
    modloader::BUILTIN_LOADERS,
};

use super::super::info::ServerInfo;
use super::git;
use super::model::{Candidate, Initialize, Initialized, Phase, Recognize};
use crate::workspace::{
    self,
    initialize::{self as local, LocalConfiguration, LocalInitialization},
};

/// Recognition never writes registration and refuses registered legacy formats.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn recognize(
    args: Recognize,
    checkpoint: impl Fn() -> Result<()>,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Vec<Candidate>> {
    checkpoint()?;
    workspace::require_local(&args.path)?;
    workspace::require_current_if_registered(&args.path)?;
    phase(Phase::Recognizing)?;
    checkpoint()?;
    let started = Instant::now();
    let candidates = if let Some(script) = args.start_script.as_deref() {
        recognize_script(&args.path, script)
    } else {
        let [run, start] = if cfg!(windows) {
            ["run.bat", "start.bat"]
        } else {
            ["run.sh", "start.sh"]
        }
        .map(Path::new);
        match recognize_script(&args.path, run) {
            Ok(candidates) => Ok(candidates),
            Err(first) => {
                checkpoint()?;
                recognize_script(&args.path, start)
                    .with_context(|| format!("{run:?} failed: {first:#}; {start:?} also failed"))
            }
        }
    }
    .context("automatic server identification failed; server remains unrecognized; initialize manually with --minecraft-version VERSION --loader forge|neoforge|fabric --loader-version VERSION, or explicitly use --loader vanilla for a known vanilla server")?;
    checkpoint()?;
    tracing::debug!(
        candidates = candidates.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "Server recognition completed without writing configuration"
    );
    let root =
        fs::canonicalize(&args.path).context("failed to resolve recognized server directory")?;
    candidates
        .into_iter()
        .map(|candidate| Candidate::from_server(candidate, &root))
        .collect()
}

fn recognize_script(path: &Path, script: &Path) -> Result<Vec<ServerCandidate>> {
    tracing::info!(path = %path.display(), script = %script.display(), "Recognizing server launch candidates");
    identify::identify(path, script, &BUILTIN_LOADERS)
}

/// Metadata is supplied in full; initialization never rereads a script or requires a recognition token.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn initialize(
    args: Initialize,
    checkpoint: impl Fn() -> Result<()>,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Initialized> {
    let started = Instant::now();
    git::validate_options(args.git, args.no_git)?;
    let result = local::initialize_local(
        LocalInitialization {
            path: args.path,
            metadata: args.metadata,
            force: args.force,
            local: args.local.map(|local| LocalConfiguration {
                configuration: local.configuration,
                replace_conf: local.replace_conf,
            }),
        },
        checkpoint,
        |stage| {
            phase(match stage {
                local::Phase::Validating => Phase::Validating,
                local::Phase::Writing => Phase::Writing,
            })
        },
        |path| {
            if args.no_git {
                Ok(None)
            } else {
                git::ensure_repository(path, args.git).map(Some)
            }
        },
    )?;
    let local::LocalInitialized {
        server,
        git,
        publication,
    } = result;
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        "Server initialization completed"
    );
    let info = ServerInfo::from_server(&server)?;
    Ok(Initialized {
        server: info,
        git,
        publication: publication.map(Into::into),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use thiserror::Error;

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
                "version":4,"minecraft_version":"1.21.1","loader":{"type":"vanilla"}
            }))
            .unwrap(),
            force,
            no_git: false,
            git: false,
            local: None,
        }
    }

    #[test]
    fn cancellation_or_consumer_failure_at_each_prewrite_phase_preserves_metadata() {
        for stop in [Phase::Validating, Phase::Writing] {
            for force in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let manifest = root.path().join(".mcpm/server.json");
                let old =
                    br#"{"version":4,"minecraft_version":"1.21.1","loader":{"type":"vanilla"}}"#;
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
                .downcast_ref::<mcpm_lib::local::mc::server::MinecraftServerError>()
                .is_some()
        );
        assert_eq!(fs::read(manifest).unwrap(), winner);
    }
}
