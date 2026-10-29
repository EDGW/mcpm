//! Caller-side adapter over the exec protocol session.
//!
//! Handlers keep their own JSON codecs, so this layer only forwards encoded values, ends the
//! frame stream at the call's completion and restores the frontend's error model.

use std::process::Command;

use anyhow::Result;

use mcpm_exec::protocol::Status;
use mcpm_exec::transport::session;

pub(crate) use mcpm_exec::protocol::Frame as Reply;

/// One protocol session with the process it started.
#[derive(Debug)]
pub(crate) struct Session(session::Session);

impl Session {
    /// Starts the process and completes the handshake.
    pub(crate) fn start(command: &mut Command) -> Result<Self> {
        session::Session::start(command, options())
            .map(Self)
            .map_err(super::command::failure)
    }

    pub(crate) fn submit(&self, operation: &str, input: serde_json::Value) -> Result<Pending> {
        self.0
            .submit(operation, input)
            .map(Pending)
            .map_err(super::command::failure)
    }

    pub(crate) fn close(&mut self) -> Result<()> {
        self.0.close().map_err(super::command::failure)
    }
}

fn options() -> session::Options {
    session::Options::new(
        crate::build_info::VERSION,
        crate::logging::level().into(),
        std::sync::Arc::new(crate::logging::replay),
    )
}

/// One call in flight; dropping it cancels the call.
#[derive(Debug)]
pub(crate) struct Pending(session::Pending);

impl Pending {
    /// The next event frame; `None` once the call completed.
    pub(crate) async fn next(&mut self) -> Result<Option<Reply>> {
        let frame = self.0.next().await.map_err(super::command::failure)?;
        Ok(reply(frame))
    }

    /// The next event frame on the current thread; `None` once the call completed.
    pub(crate) fn next_blocking(&mut self) -> Result<Option<Reply>> {
        let frame = self.0.next_blocking().map_err(super::command::failure)?;
        Ok(reply(frame))
    }

    /// Drains every event frame of this call and applies `receive` to each.
    pub(crate) fn collect(mut self, mut receive: impl FnMut(Reply) -> Result<()>) -> Result<()> {
        while let Some(reply) = self.next_blocking()? {
            receive(reply)?;
        }
        Ok(())
    }
}

fn reply(frame: Reply) -> Option<Reply> {
    match frame.status() {
        Status::Finished => None,
        _ => Some(frame),
    }
}
