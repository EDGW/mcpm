//! Deletes backup directories without collecting or removing cached content.

use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::mcmod::backup;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Delete;
impl Operation for Delete {
    const ID: &'static str = "mod.backup.delete";
    type Input = DeleteInput;
    type Output = StreamingOutput<DeleteEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let mut failed = 0;
            for path in input.backups {
                sender.checkpoint()?;
                let result = backup::delete(&path)
                    .with_context(|| format!("failed to delete backup {path:?}"));
                if result.is_err() {
                    failed += 1;
                }
                sender.send(DeleteEvent::Item {
                    path,
                    result: result.map_err(Into::into),
                })?;
            }
            sender.send(DeleteEvent::Finished { failed })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteInput {
    pub backups: Vec<PathBuf>,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum DeleteEvent {
    Item {
        path: PathBuf,
        result: Result<(), PortableError>,
    },
    Finished {
        failed: usize,
    },
}
