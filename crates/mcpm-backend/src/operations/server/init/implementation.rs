//! Read-only server recognition and independent complete-data initialization on the file-owning host.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use mcpm_exec::error::TippedError;
use mcpm_lib::local::{
    mc::server::{
        config::{self, ConfigurationOverrides, LaunchConfig},
        identify::{self, ScriptPolicy, ServerCandidate, UncertainScriptState},
        properties,
    },
    modloader::BUILTIN_LOADERS,
};
use mcpm_rich::rich;

use super::super::info::ServerInfo;
use super::RecognizeEvent;
use super::git;
use super::model::{
    Candidate, InitRconOutcome, InitRconProbe, InitRconRequest, Initialize, Initialized, Phase,
    ProbeRcon, Recognize,
};
use crate::workspace::{
    self,
    initialize::{self as local, LocalConfiguration, LocalInitialization},
};

/// Recognition never writes registration and refuses registered legacy formats.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn recognize(
    args: Recognize,
    checkpoint: impl Fn() -> Result<()>,
    emit: impl Fn(RecognizeEvent) -> Result<()>,
) -> Result<Vec<Candidate>> {
    checkpoint()?;
    workspace::require_local(&args.path)?;
    workspace::require_current_if_registered(&args.path)?;
    emit(RecognizeEvent::Stage(Phase::Recognizing))?;
    checkpoint()?;
    let started = Instant::now();
    let probe = |script| recognize_script(&args, script, &emit);
    let candidates = if let Some(script) = args.start_script.as_deref() {
        probe(script)?
    } else {
        let [run, start] = if cfg!(windows) {
            ["run.bat", "start.bat"]
        } else {
            ["run.sh", "start.sh"]
        }
        .map(Path::new);
        match probe(run)? {
            Ok(candidates) => Ok(candidates),
            Err(first) => {
                checkpoint()?;
                match probe(start)? {
                    Err(second) if first.is::<UncertainScriptState>() && !second.is::<UncertainScriptState>() => {
                        Err(first).with_context(|| format!("{start:?} also failed: {second:#}"))
                    }
                    result => result.with_context(|| format!("{run:?} failed: {first:#}; {start:?} also failed")),
                }
            }
        }
    }
    .context("automatic server identification failed; server remains unrecognized; initialize manually with --minecraft-version VERSION --loader forge|neoforge|fabric --loader-version VERSION, or explicitly use --loader vanilla for a known vanilla server")
    .map_err(|error| {
        if error.is::<UncertainScriptState>() {
            let tip = rich!("Retry with `--ignore-unknown-vars` (`-I`) if the skipped statements can safely be ignored. This keeps the previous working directory and variable values.");
            TippedError::new(error, Some(tip.clone()), Some(tip)).into()
        } else {
            error
        }
    })?;
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

fn recognize_script(
    args: &Recognize,
    script: &Path,
    emit: &impl Fn(RecognizeEvent) -> Result<()>,
) -> Result<Result<Vec<ServerCandidate>>> {
    tracing::info!(path = %args.path.display(), script = %script.display(), "Recognizing server launch candidates");
    let mut warnings = Vec::new();
    let policy = if args.ignore_unknown_vars {
        ScriptPolicy::KeepKnownState(&mut warnings)
    } else {
        ScriptPolicy::Strict
    };
    let result = identify::identify_with_policy(&args.path, script, &BUILTIN_LOADERS, policy);
    for warning in warnings {
        emit(RecognizeEvent::Warning(warning))?;
    }
    Ok(result)
}

/// Probes RCON readiness in `server.properties` on the file-owning host without mutating disk.
pub(super) fn probe_rcon(args: ProbeRcon) -> Result<InitRconProbe> {
    workspace::require_local(&args.path)?;
    let path = init_properties_path(
        &args.path,
        args.launch.as_ref(),
        args.replace_conf,
        args.preserve_conf,
        &args.conf_overrides,
    )?;
    Ok(properties::inspect_rcon_for_init(&path).into())
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
    let (local_config, mut pending_rcon) = match args.local {
        Some(local) => (
            Some(LocalConfiguration {
                configuration: local.configuration,
                replace_conf: local.replace_conf,
                conf_overrides: local.conf_overrides,
                preserve_conf: local.preserve_conf,
            }),
            local.rcon,
        ),
        None => (None, None),
    };
    if matches!(
        pending_rcon,
        Some(InitRconRequest::Explicit { .. } | InitRconRequest::InteractiveEnable { .. })
    ) {
        ensure!(
            properties::supports_rcon_properties(&args.metadata.minecraft_version),
            "enabling RCON requires Minecraft 1.9-pre4 or later; got {:?}",
            args.metadata.minecraft_version
        );
    }
    let mut rcon_outcome = None;
    let result = local::initialize_local(
        LocalInitialization {
            path: args.path,
            metadata: args.metadata,
            force: args.force,
            local: local_config.as_ref(),
        },
        checkpoint,
        |stage| {
            phase(match stage {
                local::Phase::Validating => Phase::Validating,
                local::Phase::Writing => Phase::Writing,
            })
        },
        |path| {
            if let Some(InitRconRequest::Explicit { port, password }) =
                pending_rcon.take_if(|request| matches!(request, InitRconRequest::Explicit { .. }))
            {
                let local = local_config
                    .as_ref()
                    .expect("RCON requires local configuration");
                let path = init_properties_path(
                    path,
                    local.configuration.as_ref().map(|draft| &draft.launch),
                    local.replace_conf,
                    local.preserve_conf,
                    &local.conf_overrides,
                )?;
                let plan = properties::prepare_explicit_rcon(&path, port, password)?;
                let port = properties::apply_prepared_rcon(&path, plan)?;
                rcon_outcome = Some(InitRconOutcome::Enabled {
                    port,
                    generated_password: None,
                });
            }
            Ok(())
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
    if let Some(request) = pending_rcon {
        rcon_outcome = Some(apply_interactive_rcon(server.path(), request));
    }
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        "Server initialization completed"
    );
    let info = ServerInfo::from_server(&server)?;
    Ok(Initialized {
        server: info,
        git,
        publication: publication.map(Into::into),
        rcon_outcome,
    })
}

fn init_properties_path(
    root: &Path,
    launch: Option<&LaunchConfig>,
    replace_conf: bool,
    preserve_conf: bool,
    overrides: &ConfigurationOverrides,
) -> Result<PathBuf> {
    let args =
        config::effective_conf_application_args(root, replace_conf, preserve_conf, overrides)?;
    properties::properties_path_from_launch(root, launch, &args)
}

fn apply_interactive_rcon(root: &Path, request: InitRconRequest) -> InitRconOutcome {
    let apply = || {
        let (launch, args) = config::read_properties_context(root)?;
        let path = properties::properties_path_from_launch(root, launch.as_ref(), &args)?;
        match request {
            InitRconRequest::InteractiveEnable {
                port,
                generate_password,
            } => {
                let enabled = properties::apply_interactive_enable(&path, port, generate_password)?;
                Ok(InitRconOutcome::Enabled {
                    port: enabled.port,
                    generated_password: enabled.generated_password,
                })
            }
            InitRconRequest::Disable => {
                properties::apply_disable_rcon(&path)?;
                Ok(InitRconOutcome::Disabled)
            }
            InitRconRequest::Explicit { .. } => {
                unreachable!("explicit RCON is applied before publication")
            }
        }
    };
    apply().unwrap_or_else(|error: anyhow::Error| InitRconOutcome::Warning {
        message: format!("failed to configure RCON in server.properties: {error:#}"),
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs, io};

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
    fn explicit_rcon_uses_the_probed_path_and_fails_before_registration_changes() -> Result<()> {
        use super::super::model::LocalImport;
        use crate::operations::server::info::RconInfo;

        let root = tempfile::tempdir()?;
        let root = root.path();
        let mut configuration = config::vanilla_server()?;
        configuration.launch.process.working_directory = Some("run".into());
        configuration.launch.application.extra_arguments =
            vec!["--config=unused.properties".into()];
        // Explicit conf overrides and imported settings are mutually exclusive.
        configuration.imported.clear();
        let mut overrides = ConfigurationOverrides::default();
        overrides.parse(r#"application.extraArguments:=["-c", "custom.properties"]"#)?;
        let request = |force, port| Initialize {
            local: Some(LocalImport {
                configuration: Some(configuration.clone()),
                replace_conf: force,
                conf_overrides: overrides.clone(),
                preserve_conf: false,
                rcon: Some(InitRconRequest::Explicit {
                    port,
                    password: Some("first-secret".into()),
                }),
            }),
            ..args(root, force)
        };
        let probe = || {
            probe_rcon(ProbeRcon {
                path: root.into(),
                launch: Some(configuration.launch.clone()),
                replace_conf: true,
                conf_overrides: overrides.clone(),
                preserve_conf: false,
            })
        };
        assert!(matches!(probe()?, InitRconProbe::NeedsPrompt { .. }));
        let error = initialize(request(false, None), || Ok(()), |_| Ok(())).unwrap_err();
        assert!(error.to_string().contains("--rcon-port"), "{error:#}");
        assert!(!root.join(".mcpm").exists());
        assert!(!root.join(".git").exists());
        assert!(!root.join("launch.toml").exists());
        assert!(!root.join("conf.toml").exists());

        let result = initialize(request(false, Some(25580)), || Ok(()), |_| Ok(()))?;
        assert!(matches!(
            result.server.rcon,
            Some(RconInfo::Enabled { port: 25580, .. })
        ));
        assert_eq!(probe()?, InitRconProbe::AlreadyComplete);
        let properties = root.join("run/custom.properties");
        assert!(properties.is_file());
        assert!(!root.join("server.properties").exists());
        fs::write(
            &properties,
            "enable-rcon=true\nrcon.port=0\nrcon.password=existing\n",
        )?;

        let published = [
            ".mcpm/server.json",
            ".mcpm/mcpm.json",
            "launch.toml",
            "conf.toml",
        ]
        .map(|name| root.join(name));
        let previous = published
            .iter()
            .map(fs::read)
            .collect::<io::Result<Vec<_>>>()?;
        let mut replacement = request(true, None);
        replacement.metadata.minecraft_version = "1.20.1".into();
        let error = initialize(replacement, || Ok(()), |_| Ok(())).unwrap_err();
        assert!(format!("{error:#}").contains("rcon.port"), "{error:#}");
        for (path, bytes) in published.iter().zip(previous) {
            assert_eq!(fs::read(path)?, bytes, "{path:?}");
        }
        Ok(())
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
