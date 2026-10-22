//! Command-owned reply decoding and validation; remote only routes opaque contents.

use anyhow::{Context, Result, ensure};

use crate::commands::mod_support::progress::Progress;
use crate::remote::{client::Session, command::Command};

use super::super::remote::{
    self,
    protocol::{Contents, Record},
};

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn read(
    session: &Session,
    args: remote::Args,
    progress: &Progress,
) -> Result<(Vec<Record>, bool)> {
    let started = std::time::Instant::now();
    tracing::debug!(?args, "Requesting remote mod list");
    let mut count = None;
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut done = None;
    session
        .submit(remote::List::ID, serde_json::to_value(args)?)?
        .collect(|reply| {
            let _span = tracing::debug_span!("mod_list_reply", guid = %reply.guid).entered();
            ensure!(done.is_none(), "remote list sent contents after completion");
            match reply.decode().context("invalid mod list contents")? {
                Contents::Count { count: total } => {
                    tracing::info!(count = total, "Received remote mod count");
                    ensure!(count.is_none(), "duplicate remote mod count");
                    count = Some(total);
                    progress.set_total(total);
                }
                Contents::Item { index, record } => {
                    let total = count.context("remote mod record arrived before its count")?;
                    ensure!(
                        index == records.len() && index < total,
                        "remote mod index/count mismatch"
                    );
                    tracing::debug!(index, count = total, path = %record.path, status = ?record.status, errors = record.errors.len(), "Received remote mod record");
                    records.push(record);
                    progress.advance();
                }
                Contents::Error { error } => {
                    tracing::debug!(error = %error.message, "Received remote mod list business error");
                    errors.push(error);
                },
                Contents::Done { success } => {
                    tracing::debug!(success, received = records.len(), "Received remote mod list completion");
                    done = Some(success);
                },
            }
            Ok(())
        })?;
    let success = done.context("missing remote mod list completion")?;
    if !errors.is_empty() {
        for error in errors {
            progress.report(&crate::diagnostics::RemoteError::wrap(error.into_error()));
        }
        return Err(crate::diagnostics::RemoteError::wrap(anyhow::anyhow!(
            "remote mod list failed"
        )));
    }
    ensure!(
        Some(records.len()) == count,
        "remote mod result count differs from its declared count"
    );
    tracing::info!(
        count = records.len(),
        success,
        elapsed_ms = started.elapsed().as_millis(),
        "Validated remote mod list stream"
    );
    Ok((records, success))
}
