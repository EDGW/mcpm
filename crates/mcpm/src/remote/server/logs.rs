//! Dedicated structured log subscription, drained by the server's single protocol writer.

use std::{io::Write, sync::mpsc};

use anyhow::Result;
use uuid::Uuid;

use crate::logging::stream::{Record, Subscription};

use super::super::protocol::{self, LogRequest, Reply, Status};

#[derive(Debug)]
pub(super) struct Logs {
    guid: Uuid,
    subscription: Subscription,
    records: mpsc::Receiver<Record>,
}

impl Logs {
    pub(super) fn start(request: LogRequest, writer: &mut impl Write) -> Result<Self> {
        let (subscription, records) = Subscription::start(request.level);
        protocol::write(writer, &Reply::new(request.guid, Status::Ready, ())?)?;
        tracing::debug!(guid = %request.guid, level = ?request.level, "Established remote log subscription");
        Ok(Self {
            guid: request.guid,
            subscription,
            records,
        })
    }

    /// Use a separate queue so tracing on the writer thread cannot block itself.
    pub(super) fn drain(&self, writer: &mut impl Write, limit: usize) -> Result<()> {
        for record in self.records.try_iter().take(limit) {
            protocol::write(writer, &Reply::new(self.guid, Status::Data, record)?)?;
        }
        Ok(())
    }

    pub(super) fn finish(self, writer: &mut impl Write) -> Result<()> {
        let Self {
            guid,
            subscription,
            records,
        } = self;
        drop(subscription);
        for record in records.try_iter() {
            protocol::write(writer, &Reply::new(guid, Status::Data, record)?)?;
        }
        protocol::write(writer, &Reply::new(guid, Status::Finished, ())?)
    }
}
