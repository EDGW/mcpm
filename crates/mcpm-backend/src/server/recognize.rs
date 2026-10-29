//! `server.recognize`: read-only launch recognition on the host owning the server.
//!
//! Recognition never loads registration metadata, creates directories or writes
//! configuration. Partial filters restrict candidates instead of inventing metadata.

use std::{future::Future, path::PathBuf, time::Instant};

use anyhow::Context as _;
use kako_mc_lib::local::modloader::BUILTIN_LOADERS;
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::data::server::identify;
use crate::data::workspace::{self, Kind};
use crate::error::OperationError;

use super::types::{Candidate, Phase, Recognize};

/// Recognizes server launch candidates from a start script.
#[derive(Debug)]
pub struct Recognition;

crate::host_operation!(Recognition, "server.recognize");

impl Operation for Recognition {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = Recognize;
    type Event = RecognizeEvent;
    type Output = Vec<Candidate>;
    type Error = OperationError<RecognizeKind>;

    fn run(
        input: Recognize,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || recognize(input, &emitter))
                .await
                .map_err(|error: RecognizeFailure| error.into_operation())
        }
    }
}

/// One recognition phase.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecognizeEvent {
    Stage { phase: Phase },
}

fn recognize(
    args: Recognize,
    emitter: &mcpm_exec::Emitter<RecognizeEvent>,
) -> Result<Vec<Candidate>, RecognizeFailure> {
    args.validate().map_err(RecognizeFailure::Input)?;
    emitter
        .emit(RecognizeEvent::Stage {
            phase: Phase::Recognizing,
        })
        .map_err(RecognizeFailure::Cancelled)?;
    match workspace::kind(&args.path) {
        Ok(Kind::Local) => {}
        Ok(Kind::Remote) => {
            return Err(RecognizeFailure::Remote {
                path: args.path.clone(),
            });
        }
        Err(error) => return Err(RecognizeFailure::Registration(error)),
    }
    let started = Instant::now();
    let script = args
        .start_script
        .clone()
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "run.bat" } else { "run.sh" }));
    tracing::info!(path = %args.path.display(), script = %script.display(), "Recognizing server launch candidates");
    let mut candidates = identify(&args.path, &script, &BUILTIN_LOADERS)
        .context("automatic server identification failed; server remains unrecognized; initialize manually with --minecraft-version VERSION --loader forge|neoforge|fabric --loader-version VERSION, or explicitly use --loader vanilla for a known vanilla server")
        .map_err(RecognizeFailure::Recognition)?;
    emitter
        .checkpoint()
        .map_err(RecognizeFailure::Cancelled)?;
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
        return Err(RecognizeFailure::NoMatch);
    }
    tracing::debug!(
        candidates = candidates.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "Server recognition completed without writing configuration"
    );
    Ok(candidates.into_iter().map(Candidate::from).collect())
}

#[derive(Debug, Error)]
enum RecognizeFailure {
    #[error(transparent)]
    Input(anyhow::Error),
    #[error(
        "server target {path:?} is a Remote workspace; this operation requires a Local Server (chained Remote workspaces are not supported)"
    )]
    Remote { path: PathBuf },
    #[error(transparent)]
    Registration(anyhow::Error),
    #[error(transparent)]
    Recognition(anyhow::Error),
    #[error("no identified server launch matches the supplied version/loader options; server remains unrecognized; initialize manually with complete --minecraft-version, --loader and --loader-version options (omit --loader-version only for explicitly selected vanilla)")]
    NoMatch,
    #[error("server recognition was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
}

impl RecognizeFailure {
    fn into_operation(self) -> OperationError<RecognizeKind> {
        let kind = match &self {
            Self::Input(_) => RecognizeKind::InvalidOptions,
            Self::Remote { .. } => RecognizeKind::RemoteWorkspace,
            Self::Registration(_) => RecognizeKind::Registration,
            Self::Recognition(_) | Self::NoMatch => RecognizeKind::NotRecognized,
            Self::Cancelled(_) => RecognizeKind::Cancelled,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of a recognition failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognizeKind {
    InvalidOptions,
    RemoteWorkspace,
    Registration,
    NotRecognized,
    Cancelled,
}
