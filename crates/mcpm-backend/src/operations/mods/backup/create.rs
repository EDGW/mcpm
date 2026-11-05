//! Creates full or manifest-only backups using the existing runtime cache workflow.

use super::{PublicationEvent, cache_selection};
use crate::operations::{cache::TransferPreference, stream::blocking_stream};
use anyhow::{Context, Result};
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::{mcmod::backup, runtime::RuntimeEnvironment};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, path::PathBuf};

pub struct Create;
impl Operation for Create {
    const ID: &'static str = "mod.backup.create";
    type Input = CreateInput;
    type Output = StreamingOutput<PublicationEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let environment =
                RuntimeEnvironment::new().with_backup_cache(cache_selection(input.cache_dir));
            let cache_only = matches!(input.storage, CreateStorage::CacheOnly);
            let outcome = backup::create(
                &input.source,
                &input.backup,
                &environment,
                backup::CreateOptions {
                    storage: match input.storage {
                        CreateStorage::Full(mode) => backup::CreateStorage::Full(mode.into()),
                        CreateStorage::CacheOnly => backup::CreateStorage::CacheOnly,
                    },
                    parallelism: input.parallelism,
                },
                |event| match event {
                    backup::Event::BeforeCommit => sender.checkpoint(),
                    event => sender.send(PublicationEvent::Progress(event.into())),
                },
            )
            .with_context(|| format!("failed to create backup {:?}", input.backup))?;
            let cache = if cache_only {
                Some(environment.cache.backup.open()?.path().into())
            } else {
                None
            };
            sender.send(PublicationEvent::Finished {
                outcome: (&outcome).into(),
                cache,
            })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateInput {
    pub source: PathBuf,
    pub backup: PathBuf,
    pub storage: CreateStorage,
    pub cache_dir: Option<PathBuf>,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CreateStorage {
    Full(TransferPreference),
    CacheOnly,
}
