//! Console rendering of explicit selected-file cache writes, independent of command events.

use std::{io::Write, path::Path};

use anyhow::Result;

use super::super::{
    console::ConsoleWriter,
    style::{OutputStyle, text},
};

pub(crate) struct File<'a> {
    pub path: &'a Path,
    pub id: Option<&'a str>,
    pub published: Option<bool>,
    pub method: Option<&'a str>,
    pub failed_stage: Option<&'a str>,
    pub metadata_requested: bool,
    pub metadata_status: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub identifications: Vec<(&'a str, &'static str)>,
}

pub(crate) fn file(out: &mut ConsoleWriter<'_>, file: &File<'_>) -> Result<()> {
    let style = OutputStyle::new();
    writeln!(
        out,
        "{}",
        style.muted(format!("File: {}", text(&file.path.to_string_lossy())))
    )?;
    let mut out = out.subwriter();
    if let Some(id) = file.id {
        writeln!(out, "BLAKE3: {}", style.primary(id))?;
    }
    if let Some(published) = file.published {
        writeln!(
            out,
            "Object: {}",
            if published { "added" } else { "reused" }
        )?;
    }
    if let Some(method) = file.method {
        writeln!(out, "Method: {method}")?;
    }
    if let Some(stage) = file.failed_stage {
        writeln!(out, "Result: {}", style.danger(format!("{stage} failed")))?;
    }
    if let Some(status) = file.metadata_status.filter(|_| file.metadata_requested) {
        writeln!(out, "Metadata: {status}")?;
        if let Some(reason) = file.reason {
            writeln!(out, "Reason: {}", text(reason))?;
        }
    }
    for (source, result) in &file.identifications {
        writeln!(out, "Identification ({}): {result}", text(source))?;
    }
    Ok(())
}

pub(crate) fn summary(
    out: &mut ConsoleWriter<'_>,
    files: usize,
    added: usize,
    reused: usize,
    failed: usize,
    selection_failures: usize,
) -> Result<()> {
    writeln!(out)?;
    writeln!(out, "{}", OutputStyle::new().muted(format!("{files} files · {added} objects added · {reused} objects reused · {failed} files failed · {selection_failures} selection failures")))?;
    Ok(())
}

pub(crate) fn stage_success(out: &mut ConsoleWriter<'_>, message: &str) -> Result<()> {
    writeln!(out, "{}", OutputStyle::new().success(message))?;
    Ok(())
}

/// The caller owns the record schema; this function only frames a JSON output line.
pub(crate) fn raw(out: &mut impl Write, record: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(&mut *out, record)?;
    writeln!(out)?;
    Ok(())
}
