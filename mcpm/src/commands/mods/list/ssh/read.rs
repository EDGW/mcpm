//! Command-owned reply decoding and validation; remote only routes opaque contents.

use anyhow::{Context, Result, bail, ensure};

use crate::commands::mod_support::progress::Progress;
use crate::remote::{client::Session, command::Command};

use super::super::remote::{
    self,
    protocol::{Contents, Record},
};

pub(super) fn read(
    session: &Session,
    args: remote::Args,
    progress: &Progress,
) -> Result<(Vec<Record>, bool)> {
    let mut count = None;
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut done = None;
    session
        .submit(remote::List::ID, serde_json::to_value(args)?)?
        .collect(|reply| {
            ensure!(done.is_none(), "remote list sent contents after completion");
            match reply.decode().context("invalid mod list contents")? {
                Contents::Count { count: total } => {
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
                    records.push(record);
                    progress.advance();
                }
                Contents::Error { message } => errors.push(message),
                Contents::Done { success } => done = Some(success),
            }
            Ok(())
        })?;
    let success = done.context("missing remote mod list completion")?;
    if !errors.is_empty() {
        bail!("remote mod list failed: {}", errors.join("; "));
    }
    ensure!(
        Some(records.len()) == count,
        "remote mod result count differs from its declared count"
    );
    Ok((records, success))
}
