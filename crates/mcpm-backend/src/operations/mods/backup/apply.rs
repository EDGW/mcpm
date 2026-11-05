//! Applies verified backups with the library's lazy cache fallback and staged replacement.

use super::{PublicationEvent, cache_selection};
use crate::operations::{cache::TransferPreference, stream::blocking_stream};
use anyhow::{Context, Result};
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::{cache::mods::ModsCacheDir, mcmod::backup};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, path::PathBuf};

pub struct Apply;
impl Operation for Apply {
    const ID: &'static str = "mod.backup.apply";
    type Input = ApplyInput;
    type Output = StreamingOutput<PublicationEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let selection = cache_selection(input.cache_dir);
            let outcome = backup::apply_with_cache(
                &input.backup,
                &input.target,
                || ModsCacheDir::new(selection.path()?),
                backup::ApplyOptions {
                    mode: input.mode.into(),
                    parallelism: input.parallelism,
                    force: input.force,
                },
                |event| match event {
                    backup::Event::BeforeCommit => sender.checkpoint(),
                    event => sender.send(PublicationEvent::Progress(event.into())),
                },
            )
            .with_context(|| {
                format!(
                    "failed to apply backup {:?} to {:?}",
                    input.backup, input.target
                )
            })?;
            sender.send(PublicationEvent::Finished {
                outcome: (&outcome).into(),
                cache: None,
            })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyInput {
    pub backup: PathBuf,
    pub target: PathBuf,
    pub cache_dir: Option<PathBuf>,
    pub mode: TransferPreference,
    pub force: bool,
    pub parallelism: NonZeroUsize,
}
