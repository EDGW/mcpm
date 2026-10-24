//! Console refresh details with the same content fields as cache information queries.

use std::{io::Write, path::Path};

use anyhow::Result;
use mcpm_backend::local::cache::mods::meta::CacheEntry;
use mcpm_backend::local::cache::{
    hash::Blake3Hash,
    mods::ident::{CacheIdentInfo, RefreshStatus},
};
use mcpm_backend::source::{FileMatch, SourceId};

use crate::display::{
    console::ConsoleWriter,
    source_match::{self, Column},
    style::{OutputStyle, text},
};

/// Content fields shared with info; snapshot availability remains a separate observation.
pub(crate) const COLUMNS: [super::Column; 3] = [
    super::Column::Blake3,
    super::Column::Status,
    super::Column::File,
];

pub(crate) fn short(out: &mut ConsoleWriter<'_>, id: Blake3Hash, success: bool) -> Result<()> {
    let style = OutputStyle::new();
    let result = if success {
        style.success("success")
    } else {
        style.danger("failed")
    };
    writeln!(out, "{}  {result}", style.primary(id))?;
    Ok(())
}

pub(crate) fn entry(
    out: &mut ConsoleWriter<'_>,
    content: &super::Entry,
    entry: &CacheEntry,
) -> Result<()> {
    super::write_console(out, content, &COLUMNS)?;
    if let Some(digest) = entry.digest() {
        let style = OutputStyle::new();
        writeln!(
            out,
            "{}",
            style.muted(format!("Size: {} bytes", digest.size()))
        )?;
        writeln!(
            out,
            "{}",
            style.muted(format!("SHA-256: {}", digest.sha256()))
        )?;
        for (loader, metadata) in digest.identifications() {
            crate::display::mod_meta::write(
                out,
                entry.data_path(),
                crate::display::mod_meta::View::cached(loader, metadata),
            )?;
        }
    }
    Ok(())
}

/// Renders current source outcomes and the independently observed stored positive matches.
pub(crate) fn ident_info(
    out: &mut ConsoleWriter<'_>,
    content: &super::Entry,
    info: &CacheIdentInfo,
    results: &[(SourceId, RefreshStatus)],
) -> Result<()> {
    super::write_console(out, content, &COLUMNS)?;
    let style = OutputStyle::new();
    let count = info.matches().count();
    if count == 0 {
        writeln!(out, "{}none", style.muted("Stored matches: "))?;
    } else {
        writeln!(out, "{}{count}", style.muted("Stored matches: "))?;
    }
    if count > 0 {
        writeln!(
            out,
            "{}",
            style.muted(format!(
                "Metadata: {}",
                text(&info.path().to_string_lossy())
            ))
        )?;
    }
    for (source, result) in results {
        write_ident_source(
            &mut out.subwriter(),
            source,
            Some(*result),
            info.get(&source.0),
        )?;
    }
    for (source, file) in info.matches() {
        if !results.iter().any(|(selected, _)| selected.0 == source) {
            write_ident_source(
                &mut out.subwriter(),
                &SourceId(source.into()),
                None,
                Some(file),
            )?;
        }
    }
    Ok(())
}

fn write_ident_source(
    out: &mut ConsoleWriter<'_>,
    source: &SourceId,
    result: Option<RefreshStatus>,
    file: Option<&FileMatch>,
) -> Result<()> {
    writeln!(out)?;
    let view = source_match::View::new(0, Path::new(""), source, file);
    view.write_console(out, &[Column::Source])?;
    let style = OutputStyle::new();
    if let Some(result) = result {
        let label = refresh_status(result);
        let label = match result {
            RefreshStatus::Reused | RefreshStatus::Matched => style.success(label),
            RefreshStatus::Unmatched => style.normal(label),
        };
        writeln!(out, "{}{label}", style.muted("Result: "))?;
        if result == RefreshStatus::Unmatched && file.is_some() {
            writeln!(out, "{}", style.muted("Stored match: retained"))?;
        }
    } else {
        writeln!(out, "{}", style.muted("Stored match: previously saved"))?;
    }
    if file.is_some() {
        view.write_console(out, &Column::DETAILS)?;
    }
    Ok(())
}

fn refresh_status(status: RefreshStatus) -> &'static str {
    match status {
        RefreshStatus::Reused => "reused",
        RefreshStatus::Matched => "matched",
        RefreshStatus::Unmatched => "unmatched",
    }
}
