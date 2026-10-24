//! Cache inventory tables and complete named metadata details for Console and Raw modes.

use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;
use kako_mods_lib::local::cache::mods::{CacheStatus, CacheUnavailableReason};

use super::{
    console::ConsoleWriter,
    mod_meta, source_match,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

mod model;
pub(crate) use model::{Entry, Metadata};
pub(crate) mod operations;
pub(crate) mod refresh;
pub(crate) mod write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Column {
    Blake3,
    File,
    Size,
    Sha256,
    Status,
    Metadatas,
}

impl Column {
    pub(crate) const ALL: [Self; 6] = [
        Self::Blake3,
        Self::File,
        Self::Size,
        Self::Sha256,
        Self::Status,
        Self::Metadatas,
    ];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Blake3 => "blake3",
            Self::File => "file",
            Self::Size => "size",
            Self::Sha256 => "sha256",
            Self::Status => "status",
            Self::Metadatas => "metadatas",
        }
    }
    pub(crate) fn letter(self) -> char {
        match self {
            Self::Blake3 => 'B',
            Self::File => 'F',
            Self::Size => 'S',
            Self::Sha256 => 'H',
            Self::Status => 'T',
            Self::Metadatas => 'M',
        }
    }
}

pub(crate) fn headers(out: &mut impl Write, columns: &[Column], enabled: bool) -> Result<()> {
    if enabled && !terminal::stdout_is_tty() {
        writeln!(
            out,
            "{}",
            columns
                .iter()
                .map(|column| column.name())
                .collect::<Vec<_>>()
                .join("\t")
        )?;
    }
    Ok(())
}

pub(crate) fn list_entry(out: &mut impl Write, entry: &Entry, columns: &[Column]) -> Result<()> {
    if terminal::stdout_is_tty() {
        let mut out = ConsoleWriter::new(out);
        if columns.len() == 1 {
            return write_single(&mut out, entry, columns[0]);
        }
        return write_console(&mut out, entry, columns);
    }
    let values = columns
        .iter()
        .map(|column| {
            tsv(&match column {
                Column::Blake3 => entry.id.to_string(),
                Column::File => entry.file.to_string_lossy().into_owned(),
                Column::Size => entry.size.map(|size| size.to_string()).unwrap_or_default(),
                Column::Sha256 => entry.sha256.clone().unwrap_or_default(),
                Column::Status => entry.status.to_owned(),
                Column::Metadatas => entry
                    .metadatas
                    .iter()
                    .map(Metadata::name)
                    .collect::<Vec<_>>()
                    .join(","),
            })
        })
        .collect::<Vec<_>>();
    writeln!(out, "{}", values.join("\t"))?;
    Ok(())
}

pub(crate) fn single_header(out: &mut impl Write, column: Column, enabled: bool) -> Result<()> {
    if enabled {
        let mut out = ConsoleWriter::new(out);
        writeln!(out, "{}", OutputStyle::new().muted(column.name()))?;
    }
    Ok(())
}

fn write_single(out: &mut ConsoleWriter<'_>, entry: &Entry, column: Column) -> Result<()> {
    let style = OutputStyle::new();
    match column {
        Column::Blake3 => writeln!(out, "{}", style.primary(entry.id))?,
        Column::File => writeln!(out, "{}", style.muted(text(&entry.file.to_string_lossy())))?,
        Column::Size => writeln!(
            out,
            "{}",
            entry.size.map(|size| size.to_string()).unwrap_or_default()
        )?,
        Column::Sha256 => writeln!(
            out,
            "{}",
            style.muted(entry.sha256.as_deref().unwrap_or(""))
        )?,
        Column::Status => writeln!(out, "{}", entry.status)?,
        Column::Metadatas => write_metadatas(out, entry)?,
    }
    Ok(())
}

pub(crate) fn info(out: &mut impl Write, entry: &Entry, columns: &[Column]) -> Result<()> {
    if !terminal::stdout_is_tty() {
        return list_entry(out, entry, columns);
    }
    write_console(&mut ConsoleWriter::new(out), entry, columns)
}

fn write_console(out: &mut ConsoleWriter<'_>, entry: &Entry, columns: &[Column]) -> Result<()> {
    let style = OutputStyle::new();
    for column in columns {
        match column {
            Column::Blake3 => writeln!(
                out,
                "{}{}",
                style.muted("BLAKE3: "),
                style.primary(entry.id)
            )?,
            Column::File => writeln!(
                out,
                "{}",
                style.muted(format!("File: {}", text(&entry.file.to_string_lossy())))
            )?,
            Column::Size => writeln!(
                out,
                "{}",
                style.muted(format!(
                    "Size: {}",
                    entry
                        .size
                        .map(|size| format!("{size} bytes"))
                        .unwrap_or_else(|| "unavailable".into())
                ))
            )?,
            Column::Sha256 => writeln!(
                out,
                "{}",
                style.muted(format!(
                    "SHA-256: {}",
                    entry.sha256.as_deref().unwrap_or("unavailable")
                ))
            )?,
            Column::Status => {
                let status = if entry.status == "available" {
                    style.success(entry.status)
                } else {
                    style.warning(entry.status)
                };
                writeln!(out, "{}{status}", style.muted("Status: "))?;
                if let Some(reason) = &entry.reason {
                    writeln!(out, "{}", style.muted(format!("Reason: {}", text(reason))))?;
                }
            }
            Column::Metadatas => {
                writeln!(
                    out,
                    "{}",
                    style.muted(format!("Metadatas: {}", entry.metadatas.len()))
                )?;
                write_metadatas(out, entry)?;
            }
        }
    }
    Ok(())
}

fn write_metadatas(out: &mut ConsoleWriter<'_>, entry: &Entry) -> Result<()> {
    let style = OutputStyle::new();
    let mut out = out.subwriter();
    for metadata in &entry.metadatas {
        writeln!(out, "{}", style.secondary(metadata.name()))?;
        let mut details = out.subwriter();
        match metadata {
            Metadata::Meta(meta) => {
                let (status, reason) = state(meta.status());
                writeln!(details, "{status}")?;
                if let Some(reason) = reason {
                    writeln!(details, "{}", style.muted(text(reason)))?;
                }
                if meta.status() == &CacheStatus::Available
                    && let Some(digest) = meta.digest()
                {
                    for (loader, result) in digest.identifications() {
                        mod_meta::write(
                            &mut details,
                            &entry.file,
                            mod_meta::View::cached(loader, result),
                        )?;
                    }
                }
            }
            Metadata::Ident(info) => {
                let (status, reason) = state(info.status());
                writeln!(details, "{status}")?;
                if let Some(reason) = reason {
                    writeln!(details, "{}", style.muted(text(reason)))?;
                }
                for (source, file) in info.matches() {
                    writeln!(details)?;
                    source_match::View::new(
                        0,
                        &entry.file,
                        &kako_mods_lib::source::SourceId(source.into()),
                        Some(file),
                    )
                    .write_console(
                        &mut details,
                        &[
                            source_match::Column::Source,
                            source_match::Column::Project,
                            source_match::Column::Id,
                            source_match::Column::Filename,
                            source_match::Column::Size,
                            source_match::Column::Role,
                        ],
                    )?;
                }
            }
            Metadata::Other(_) => writeln!(details, "{}", style.muted("present"))?,
        }
    }
    Ok(())
}

fn state(status: &CacheStatus) -> (&'static str, Option<&str>) {
    match status {
        CacheStatus::Available => ("available", None),
        CacheStatus::NotFound => ("not_found", None),
        CacheStatus::Unavailable(reason) => (
            "unavailable",
            Some(match reason {
                CacheUnavailableReason::MissingDigest => "missing_digest",
                CacheUnavailableReason::InvalidData => "invalid_data",
                CacheUnavailableReason::UnexpectedError(_) => "unexpected_error",
            }),
        ),
    }
}
