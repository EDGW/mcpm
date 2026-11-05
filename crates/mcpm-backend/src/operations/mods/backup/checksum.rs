//! Sequential directory checksums with per-directory failures and original backup hashing rules.

use super::{DirectoryChecksum, Progress};
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::mcmod::backup;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, path::PathBuf};

pub struct Checksum;
impl Operation for Checksum {
    const ID: &'static str = "mod.backup.checksum";
    type Input = ChecksumInput;
    type Output = StreamingOutput<ChecksumEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let mut failed = 0;
            for path in input.directories {
                sender.checkpoint()?;
                let result = backup::checksum(&path, input.parallelism, |event| {
                    sender.send(ChecksumEvent::Progress(event.into()))
                })
                .with_context(|| format!("failed to compute directory checksum for {path:?}"));
                if result.is_err() {
                    failed += 1;
                }
                sender.send(ChecksumEvent::Item {
                    path,
                    result: result.map(|value| (&value).into()).map_err(Into::into),
                })?;
            }
            sender.send(ChecksumEvent::Finished { failed })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChecksumInput {
    pub directories: Vec<PathBuf>,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum ChecksumEvent {
    Progress(Progress),
    Item {
        path: PathBuf,
        result: Result<DirectoryChecksum, PortableError>,
    },
    Finished {
        failed: usize,
    },
}
