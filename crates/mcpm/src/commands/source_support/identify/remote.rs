//! Command-owned source-identification registration, structured arguments and reply validation.

use anyhow::{Context, Result, ensure};
use mcpm_backend::local::runtime::RuntimeEnvironment;
use mcpm_backend::source::SummaryRequirements;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{num::NonZeroUsize, path::PathBuf};

use crate::remote::{
    client::{Pending, Session},
    command::{Command, Output},
};

use crate::commands::execution::{DEFAULT_PARALLELISM, Runtime};

use super::summary::{self, Event, Outcome};

#[derive(Debug)]
struct Identify;
impl Command for Identify {
    const ID: &'static str = "mod.source.identify";

    #[tracing::instrument(
        name = "source_identify",
        level = "info",
        skip_all,
        fields(execution = "remote", phase = "summary"),
        err(level = "debug")
    )]
    fn run(contents: Value, output: &Output) -> Result<()> {
        tracing::info!("Starting remote file summary command");
        let args: Request =
            serde_json::from_value(contents).context("invalid source identify arguments")?;
        args.validate()?;
        tracing::debug!(?args, "Decoded remote source identify arguments");
        let runtime = Runtime::new()?;
        let environment = RuntimeEnvironment::new().with_mods_cache(args.cache);
        let result = runtime.block_on(async {
            summary::run(
                args.paths,
                args.requirements,
                args.parallelism.get(),
                &environment,
                || output.checkpoint(),
                |event| std::future::ready(output.send(event)),
            )
            .await
        });
        tracing::debug!(
            success = result.is_ok(),
            "Remote file summary command ended"
        );
        result
    }
}

/// No provider configuration or API keys cross the SSH boundary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub paths: Vec<PathBuf>,
    pub requirements: SummaryRequirements,
    #[serde(default = "default_parallelism")]
    pub parallelism: NonZeroUsize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<mcpm_backend::local::cache::mods::ModsCacheSelection>,
}
impl Request {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.paths.is_empty(),
            "source identify requires at least one path"
        );
        Ok(())
    }
}
fn default_parallelism() -> NonZeroUsize {
    NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap()
}

#[derive(Debug)]
pub(super) struct Lookup {
    pending: Pending,
    states: Vec<u8>,
    failed: usize,
    done: bool,
    requirements: SummaryRequirements,
    started: std::time::Instant,
}
impl Lookup {
    pub(super) fn start(session: &Session, args: &Request) -> Result<Self> {
        args.validate()?;
        tracing::debug!(
            ?args,
            "Requesting remote file summaries on existing session"
        );
        Ok(Self {
            pending: session.submit(Identify::ID, serde_json::to_value(args)?)?,
            states: vec![0; args.paths.len()],
            failed: 0,
            done: false,
            requirements: args.requirements,
            started: std::time::Instant::now(),
        })
    }

    /// Validate business state independently of generic GUID/status routing.
    pub(super) async fn next(&mut self) -> Result<Option<Event>> {
        let Some(reply) = self.pending.next().await? else {
            ensure!(self.done, "missing remote source identify completion");
            return Ok(None);
        };
        ensure!(!self.done, "remote source identify data after completion");
        let event: Event = reply.decode().context("invalid source identify contents")?;
        tracing::trace!(?event, "Received source identify event");
        match &event {
            Event::Loading { indices } | Event::Item { indices, .. } => {
                ensure!(!indices.is_empty(), "empty remote summary indices");
                let expected = u8::from(matches!(event, Event::Item { .. }));
                for index in indices {
                    let state = self
                        .states
                        .get_mut(*index)
                        .context("remote path index out of range")?;
                    ensure!(
                        *state == expected,
                        "duplicate or out-of-order remote summary item"
                    );
                    *state += 1;
                }
                if let Event::Item { outcome, .. } = &event {
                    match outcome {
                        Outcome::Failed { .. } => self.failed += indices.len(),
                        Outcome::Summary { summary, cached } => {
                            for (key, file) in cached {
                                ensure!(
                                    !key.is_empty()
                                        && !key.contains('@')
                                        && file.reference.project.source.0 == *key,
                                    "invalid cached source identity"
                                );
                                file.validate(summary)?;
                            }
                            summary.validate()?;
                            ensure!(
                                !self.requirements.murmur2 || summary.murmur2.is_some(),
                                "remote summary omitted required MurmurHash2 fingerprint"
                            );
                        }
                    }
                }
            }
            Event::Done { failed } => {
                ensure!(
                    self.states.iter().all(|state| *state == 2),
                    "remote identification result count mismatch"
                );
                ensure!(
                    *failed == self.failed,
                    "remote identification failure count mismatch"
                );
                self.done = true;
                tracing::debug!(
                    failed,
                    elapsed_ms = self.started.elapsed().as_millis(),
                    "Validated remote file summaries"
                );
            }
        }
        Ok(Some(event))
    }
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Identify>()
}
inventory::submit! { crate::initcall::Initcall(init) }
