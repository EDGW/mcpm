//! Sequential self-contained backup verification, continuing after independent backup failures.

use super::{Progress, VerifiedBackup};
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::mcmod::backup;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, path::PathBuf};

pub struct Verify;
impl Operation for Verify {
    const ID: &'static str = "mod.backup.verify";
    type Input = VerifyInput;
    type Output = StreamingOutput<VerifyEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let mut failed = 0;
            for path in input.backups {
                sender.checkpoint()?;
                let result = backup::verify(&path, input.parallelism, |event| {
                    sender.send(VerifyEvent::Progress(event.into()))
                })
                .with_context(|| format!("failed to verify backup {path:?}"));
                if result.is_err() {
                    failed += 1;
                }
                sender.send(VerifyEvent::Item {
                    path,
                    result: result.map(|value| (&value).into()).map_err(Into::into),
                })?;
            }
            sender.send(VerifyEvent::Finished { failed })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyInput {
    pub backups: Vec<PathBuf>,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum VerifyEvent {
    Progress(Progress),
    Item {
        path: PathBuf,
        result: Result<VerifiedBackup, PortableError>,
    },
    Finished {
        failed: usize,
    },
}
