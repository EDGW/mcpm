//! Computes a directory once, then compares ordered backup or checksum targets on its Host.

use super::{Comparison, DirectoryChecksum, Progress};
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::mcmod::backup;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, path::PathBuf};

pub struct VerifyWith;
impl Operation for VerifyWith {
    const ID: &'static str = "mod.backup.verify_with";
    type Input = VerifyWithInput;
    type Output = StreamingOutput<VerifyWithEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let computed = backup::checksum(&input.directory, input.parallelism, |event| {
                sender.send(VerifyWithEvent::Progress(event.into()))
            })
            .with_context(|| {
                format!(
                    "failed to compute directory checksum for {:?}",
                    input.directory
                )
            });
            sender.send(VerifyWithEvent::Computed(
                computed.as_ref().ok().map(Into::into),
            ))?;
            let mut failed = 0;
            let mut mismatched = 0;
            for target in input.targets {
                sender.checkpoint()?;
                let result = computed
                    .as_ref()
                    .ok()
                    .map(|computed| target.compare(computed));
                match &result {
                    Some(Ok(comparison)) if !comparison.matches() => mismatched += 1,
                    Some(Err(_)) | None => failed += 1,
                    Some(Ok(_)) => {}
                }
                sender.send(VerifyWithEvent::Item {
                    target,
                    result: result
                        .map(|result| result.map(|value| (&value).into()).map_err(Into::into)),
                })?;
            }
            if let Err(error) = computed {
                sender.send(VerifyWithEvent::ComputationFailed(error.into()))?;
            }
            sender.send(VerifyWithEvent::Finished { failed, mismatched })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyWithInput {
    pub directory: PathBuf,
    pub targets: Vec<Target>,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum VerifyWithEvent {
    Progress(Progress),
    Computed(Option<DirectoryChecksum>),
    Item {
        target: Target,
        result: Option<Result<Comparison, PortableError>>,
    },
    ComputationFailed(PortableError),
    Finished {
        failed: usize,
        mismatched: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Backup(PathBuf),
    Checksum(String),
}
impl Target {
    fn compare(&self, computed: &backup::DirectoryChecksum) -> Result<backup::Comparison> {
        match self {
            Self::Backup(path) => computed
                .compare_with_backup(path)
                .with_context(|| format!("failed to compare with backup {path:?}")),
            Self::Checksum(value) => Ok(computed.compare_checksum(
                value
                    .parse()
                    .with_context(|| format!("invalid expected checksum {value:?}"))?,
            )),
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Backup(_) => "backup",
            Self::Checksum(_) => "checksum",
        }
    }
    pub fn value(&self) -> String {
        match self {
            Self::Backup(path) => path.to_string_lossy().into_owned(),
            Self::Checksum(value) => value.clone(),
        }
    }
}
