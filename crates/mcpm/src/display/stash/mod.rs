//! Shared stash fields, workspace differences and operation results in Console and Raw TSV.

use std::{io::Write, path::PathBuf};

use anyhow::Result;
use clap::ValueEnum;
use mcpm_backend::local::{
    mc::server::stash::{Stash, Status, WorkspaceState},
    mcmod::backup::{FileDifference, Manifest},
};

use super::{
    backup,
    console::ConsoleWriter,
    size::size,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

/// List columns are lightweight; manifest-derived fields belong to info only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Column {
    Name,
    Id,
    Time,
    Head,
    Backup,
    Path,
    Checksum,
    Files,
    Size,
}

impl Column {
    pub(crate) const LIST: [Self; 5] = [Self::Name, Self::Id, Self::Time, Self::Head, Self::Backup];
    pub(crate) const ALL: [Self; 9] = [
        Self::Name,
        Self::Id,
        Self::Time,
        Self::Head,
        Self::Backup,
        Self::Path,
        Self::Checksum,
        Self::Files,
        Self::Size,
    ];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Id => "id",
            Self::Time => "time",
            Self::Head => "head",
            Self::Backup => "backup",
            Self::Path => "path",
            Self::Checksum => "checksum",
            Self::Files => "files",
            Self::Size => "size",
        }
    }
    pub(crate) fn letter(self) -> char {
        match self {
            Self::Name => 'N',
            Self::Id => 'I',
            Self::Time => 'T',
            Self::Head => 'H',
            Self::Backup => 'B',
            Self::Path => 'P',
            Self::Checksum => 'C',
            Self::Files => 'F',
            Self::Size => 'S',
        }
    }
    pub(crate) fn needs_manifest(self) -> bool {
        matches!(self, Self::Checksum | Self::Files | Self::Size)
    }
}

/// Captured under a shared lease; rendering does not access server files.
pub(crate) struct Entry {
    pub stash: Stash,
    pub head: Option<bool>,
    pub backup: PathBuf,
    pub manifest: Option<Manifest>,
}

impl Entry {
    fn value(&self, column: Column) -> String {
        match column {
            Column::Name => self.stash.name().into(),
            Column::Id => self.stash.id().to_string(),
            Column::Time => self.stash.created_at().to_rfc3339(),
            Column::Head => self.head.map(|head| head.to_string()).unwrap_or_default(),
            Column::Backup => self.stash.backup_id().to_string(),
            Column::Path => self.backup.to_string_lossy().into_owned(),
            Column::Checksum => self
                .manifest
                .as_ref()
                .map(|m| m.checksum().to_string())
                .unwrap_or_default(),
            Column::Files => self
                .manifest
                .as_ref()
                .map(|m| m.files().len().to_string())
                .unwrap_or_default(),
            Column::Size => self
                .manifest
                .as_ref()
                .map(|m| {
                    m.files()
                        .iter()
                        .map(|f| u128::from(f.size()))
                        .sum::<u128>()
                        .to_string()
                })
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn entries(
    out: &mut impl Write,
    entries: &[Entry],
    columns: &[Column],
    headers: bool,
    details: bool,
    labeled: bool,
) -> Result<()> {
    if !terminal::stdout_is_tty() {
        if headers {
            writeln!(
                out,
                "{}",
                columns
                    .iter()
                    .map(|c| c.name())
                    .collect::<Vec<_>>()
                    .join("\t")
            )?;
        }
        for entry in entries {
            writeln!(
                out,
                "{}",
                columns
                    .iter()
                    .map(|column| tsv(&entry.value(*column)))
                    .collect::<Vec<_>>()
                    .join("\t")
            )?;
        }
        return Ok(());
    }
    let style = OutputStyle::new();
    let mut console = ConsoleWriter::new(out);
    if labeled && columns.len() == 1 {
        writeln!(console, "{}:", columns[0].name())?;
    }
    for (index, entry) in entries.iter().enumerate() {
        if labeled {
            if index > 0 && columns.len() > 1 {
                writeln!(console)?;
            }
            for column in columns {
                let value = entry.value(*column);
                let value = match column {
                    Column::Name => style.primary(text(&value)),
                    Column::Time | Column::Head => style.secondary(text(&value)),
                    Column::Size => entry
                        .manifest
                        .as_ref()
                        .map(|m| size(m.files().iter().map(|f| u128::from(f.size())).sum(), &style))
                        .unwrap_or_default(),
                    _ => style.muted(text(&value)),
                };
                if columns.len() == 1 {
                    writeln!(console, "{value}")?;
                } else {
                    writeln!(console, "{}: {value}", column.name())?;
                }
            }
        } else {
            writeln!(
                console,
                "{}  {}  {}",
                style.primary(text(entry.stash.name())),
                style.secondary(entry.stash.created_at()),
                if entry.head == Some(true) {
                    style.secondary("HEAD")
                } else {
                    String::new()
                }
            )?;
        }
        if details && let Some(manifest) = &entry.manifest {
            let mut files = console.subwriter();
            for record in manifest.files() {
                backup::file(&mut files, record, None)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn operation(
    out: &mut impl Write,
    operation: &str,
    stash: Option<&Stash>,
    target: &str,
    status: &str,
    headers: bool,
) -> Result<()> {
    if !terminal::stdout_is_tty() {
        if headers {
            writeln!(out, "operation\ttarget\tid\tname\tstatus")?;
        }
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}",
            tsv(operation),
            tsv(target),
            stash.map(|s| s.id().to_string()).unwrap_or_default(),
            stash.map(|s| tsv(s.name())).unwrap_or_default(),
            tsv(status)
        )?;
    } else {
        let style = OutputStyle::new();
        let name = stash.map(|s| s.name()).unwrap_or(target);
        let status = if status == "failed" {
            style.danger(status)
        } else {
            style.success(status)
        };
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "{operation}: {}  {status}",
            style.primary(text(name))
        )?;
        if let Some(stash) = stash {
            writeln!(console.subwriter(), "{}", style.muted(stash.id()))?;
        }
    }
    Ok(())
}

pub(crate) fn status(out: &mut impl Write, status: &Status, headers: bool) -> Result<()> {
    let state = match status.state() {
        WorkspaceState::Clean => "clean",
        WorkspaceState::Modified => "modified",
        WorkspaceState::Untracked => "untracked",
    };
    let head_id = status
        .head
        .as_ref()
        .map(|s| s.id().to_string())
        .unwrap_or_default();
    let head_name = status.head.as_ref().map(|s| s.name()).unwrap_or_default();
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "Status: {}",
            if status.state() == WorkspaceState::Clean {
                style.success(state)
            } else {
                style.warning(state)
            }
        )?;
        if status.head.is_some() {
            writeln!(console, "HEAD: {}", style.primary(text(head_name)))?;
            writeln!(console.subwriter(), "{}", style.muted(head_id))?;
        }
        if let Some(comparison) = &status.comparison {
            for difference in comparison.differences() {
                backup::file_difference(&mut console, difference)?;
            }
        }
    } else {
        if headers {
            writeln!(
                out,
                "state\tid\tname\texpected_checksum\tactual_checksum\tchange\tpath"
            )?;
        }
        let expected = status
            .comparison
            .as_ref()
            .map(|c| c.expected().to_string())
            .unwrap_or_default();
        let actual = status
            .comparison
            .as_ref()
            .map(|c| c.actual().to_string())
            .unwrap_or_default();
        let prefix = format!(
            "{state}\t{head_id}\t{}\t{expected}\t{actual}",
            tsv(head_name)
        );
        let differences = status
            .comparison
            .as_ref()
            .map(|c| c.differences())
            .unwrap_or_default();
        if differences.is_empty() {
            writeln!(out, "{prefix}\t\t")?;
        }
        for difference in differences {
            let (change, record) = match difference {
                FileDifference::Missing { expected } => ("removed", expected),
                FileDifference::Unexpected { actual } => ("added", actual),
                FileDifference::Changed { actual, .. } => ("modified", actual),
            };
            writeln!(out, "{prefix}\t{change}\t{}", tsv(record.path()))?;
        }
    }
    Ok(())
}
