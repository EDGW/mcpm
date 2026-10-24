//! TSV receipts and styled Console output for cache operations without metadata inspection.

use std::{io::Write, path::Path};

use anyhow::Result;
use kako_mods_lib::local::cache::{ImportOutcome, TransferMethod, hash::Blake3Hash};

use super::super::{
    console::ConsoleWriter,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

fn row(
    out: &mut impl Write,
    headings: &[&str],
    values: &[String],
    headers: bool,
    console: String,
) -> Result<()> {
    if terminal::stdout_is_tty() {
        let mut out = ConsoleWriter::new(out);
        writeln!(out, "{console}")?;
    } else {
        if headers {
            writeln!(out, "{}", headings.join("\t"))?;
        }
        writeln!(
            out,
            "{}",
            values
                .iter()
                .map(|value| tsv(value))
                .collect::<Vec<_>>()
                .join("\t")
        )?;
    }
    Ok(())
}

pub(crate) fn method(method: TransferMethod) -> &'static str {
    match method {
        TransferMethod::Copy => "copy",
        TransferMethod::HardLink => "hard_link",
    }
}

pub(crate) fn initialized(out: &mut impl Write, path: &Path, headers: bool) -> Result<()> {
    row(
        out,
        &["directory", "version"],
        &[
            path.to_string_lossy().into_owned(),
            kako_mods_lib::local::cache::codec::CACHE_DIRECTORY_VERSION.to_string(),
        ],
        headers,
        format!(
            "Initialized: {}",
            OutputStyle::new().muted(text(&path.to_string_lossy()))
        ),
    )
}

pub(crate) fn imported(
    out: &mut impl Write,
    path: &Path,
    outcome: ImportOutcome,
    headers: bool,
) -> Result<()> {
    let state = if outcome.published { "added" } else { "reused" };
    row(
        out,
        &["blake3", "file", "published", "method"],
        &[
            outcome.key.to_string(),
            path.to_string_lossy().into_owned(),
            outcome.published.to_string(),
            outcome.method.map(method).unwrap_or("").into(),
        ],
        headers,
        format!(
            "{}  {state}  {}",
            OutputStyle::new().primary(outcome.key),
            OutputStyle::new().muted(text(&path.to_string_lossy()))
        ),
    )
}

pub(crate) fn exported(
    out: &mut impl Write,
    id: Blake3Hash,
    path: &Path,
    transfer: TransferMethod,
    headers: bool,
) -> Result<()> {
    row(
        out,
        &["blake3", "destination", "method"],
        &[
            id.to_string(),
            path.to_string_lossy().into_owned(),
            method(transfer).into(),
        ],
        headers,
        format!(
            "{}  {} -> {}",
            OutputStyle::new().primary(id),
            method(transfer),
            OutputStyle::new().muted(text(&path.to_string_lossy()))
        ),
    )
}

pub(crate) fn removed(
    out: &mut impl Write,
    id: Blake3Hash,
    removed: bool,
    headers: bool,
) -> Result<()> {
    row(
        out,
        &["blake3", "removed"],
        &[id.to_string(), removed.to_string()],
        headers,
        format!(
            "{}  {}",
            OutputStyle::new().primary(id),
            if removed { "removed" } else { "not found" }
        ),
    )
}

pub(crate) fn verification(
    out: &mut impl Write,
    id: Blake3Hash,
    status: &str,
    reason: &str,
    headers: bool,
) -> Result<()> {
    row(
        out,
        &["blake3", "status", "reason"],
        &[id.to_string(), status.into(), reason.into()],
        headers,
        format!(
            "{}  {}{}",
            OutputStyle::new().primary(id),
            if status == "available" {
                OutputStyle::new().success(status)
            } else {
                OutputStyle::new().warning(status)
            },
            if reason.is_empty() {
                String::new()
            } else {
                format!(" ({})", text(reason))
            }
        ),
    )
}

pub(crate) fn rebuilt_index(out: &mut impl Write, count: usize, headers: bool) -> Result<()> {
    row(
        out,
        &["indexed"],
        &[count.to_string()],
        headers,
        format!("Index rebuilt: {count} objects"),
    )
}
