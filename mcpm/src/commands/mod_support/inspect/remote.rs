//! Shared SSH inspection consumption, with validated events and local presentation.

use super::{Display, Event, Presentation, Validator};
use crate::ssh::SshArgs;
use anyhow::{Context, Result};
use serde_json::Value;

pub(in crate::commands) fn run_ssh(
    ssh: SshArgs,
    command: &str,
    contents: Value,
    display: Display<'_>,
) -> Result<()> {
    tracing::info!(command, "Opening SSH data source for mod inspection");
    let mut session = ssh.connect()?;
    tracing::debug!(command, "Requesting remote mod inspection");
    let mut presentation = Presentation::new(display, true);
    let mut validator = Validator::default();
    session.submit(command, contents)?.collect(|reply| {
        let event: Event = reply.decode().context("invalid remote mod inspection contents")?;
        validator.accept(&event)?;
        if let Event::File { file, .. } = &event {
            tracing::debug!(path = %file.path, loaders = file.metadata.len(), "Received remote inspected mod file");
        }
        presentation.event(event)
    })?;
    validator.finish()?;
    session.close()?;
    presentation.finish()
}
