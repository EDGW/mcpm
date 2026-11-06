//! Run-owned command outcomes: relative-path storage, lifetime deduplication and bounded responses.

use super::{publish, read_optional};
use crate::operations::server::shim::{Channel, CommandOutcome, CommandResult, CommandStatus};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSES: usize = 256;
pub(super) struct History {
    directory: PathBuf,
    responses: VecDeque<(PathBuf, usize)>,
    bytes: usize,
}
impl History {
    pub fn create(directory: &Path, run_id: &str) -> Result<Self> {
        let directory = directory.join(format!("commands-{run_id}"));
        std::fs::create_dir(&directory).context("failed to create command history")?;
        Ok(Self {
            directory,
            responses: VecDeque::new(),
            bytes: 0,
        })
    }
    /// Claims an ID before any command is sent. A retained ID can never execute again.
    pub fn prepare(&self, id: String, digest: String, initial: &CommandResult) -> Result<Prepared> {
        let path = self
            .directory
            .join(format!("{}.json", blake3::hash(id.as_bytes()).to_hex()));
        if let Some(previous) = read_optional::<Record>(&path)? {
            ensure!(
                previous.id == id,
                "command identifier hash collision; refusing execution"
            );
            ensure!(
                previous.digest == digest,
                "command id reused for different command text"
            );
            return Ok(Prepared::Existing(previous.result));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        serde_json::to_writer(
            &mut file,
            &Record {
                id: &id,
                digest: &digest,
                result: initial,
            },
        )?;
        file.flush()?;
        Ok(Prepared::Pending(Ticket { path, id, digest }))
    }
    pub fn complete(&mut self, ticket: Ticket, result: &CommandResult) -> Result<()> {
        publish(
            &ticket.path,
            &Record {
                id: &ticket.id,
                digest: &ticket.digest,
                result,
            },
        )?;
        let bytes = result.output.as_ref().map_or(0, String::len);
        if bytes > 0 {
            self.bytes += bytes;
            self.responses.push_back((ticket.path, bytes));
        }
        while self.bytes > MAX_BYTES || self.responses.len() > MAX_RESPONSES {
            let (path, bytes) = self
                .responses
                .pop_front()
                .expect("retained response exceeds limit");
            let mut previous: Record =
                read_optional(&path)?.context("retained command record is missing")?;
            previous.result = CommandResult {
                status: CommandStatus::Accepted,
                channel: Channel::Rcon,
                output: None,
                result: CommandOutcome::Unknown,
                diagnostic: Some("retained response expired; command was not replayed".into()),
            };
            publish(&path, &previous)?;
            self.bytes -= bytes;
        }
        Ok(())
    }
    pub fn remove(self) -> Result<()> {
        std::fs::remove_dir_all(&self.directory)
            .context("failed to remove completed command history")
    }
}

pub(super) enum Prepared {
    Existing(CommandResult),
    Pending(Ticket),
}
pub(super) struct Ticket {
    path: PathBuf,
    id: String,
    digest: String,
}
#[derive(Serialize, Deserialize)]
struct Record<I = String, D = String, R = CommandResult> {
    id: I,
    digest: D,
    result: R,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> CommandResult {
        CommandResult {
            status: CommandStatus::Accepted,
            channel: Channel::Rcon,
            output: None,
            result: CommandOutcome::Unknown,
            diagnostic: None,
        }
    }
    #[test]
    fn expired_output_and_pending_requests_never_become_new_commands() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut history = History::create(directory.path(), "test")?;
        let ticket = match history.prepare("same".into(), "digest".into(), &pending())? {
            Prepared::Pending(ticket) => ticket,
            _ => panic!("first request already exists"),
        };
        let Prepared::Existing(unconfirmed) =
            history.prepare("same".into(), "digest".into(), &pending())?
        else {
            panic!("pending command could replay");
        };
        assert_eq!(unconfirmed.result, CommandOutcome::Unknown);
        assert!(
            history
                .prepare("same".into(), "different".into(), &pending())
                .is_err()
        );
        let mut output = pending();
        output.output = Some("x".repeat(MAX_BYTES + 1));
        output.result = CommandOutcome::Completed;
        history.complete(ticket, &output)?;
        let Prepared::Existing(evicted) =
            history.prepare("same".into(), "digest".into(), &pending())?
        else {
            panic!("evicted command could replay");
        };
        assert_eq!(evicted.result, CommandOutcome::Unknown);
        assert!(evicted.output.is_none());
        history.remove()?;
        Ok(())
    }
}
