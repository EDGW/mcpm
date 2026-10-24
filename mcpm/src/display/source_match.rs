//! Shared field groups and escaped TSV columns for source-identification results.

use std::{
    io::{self, Write},
    path::Path,
};

use mcpm_backend::source::{FileMatch, MatchedFileRole, SourceId};

use clap::ValueEnum;

use crate::{
    display::{
        console::ConsoleWriter,
        style::{OutputStyle, text, tsv},
    },
    terminal,
};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum, Debug)]
pub(crate) enum Column {
    Index,
    Path,
    Source,
    Project,
    Id,
    Filename,
    Size,
    Role,
    Status,
}

impl Column {
    pub(crate) const ALL: [Self; 9] = [
        Self::Index,
        Self::Path,
        Self::Source,
        Self::Project,
        Self::Id,
        Self::Filename,
        Self::Size,
        Self::Role,
        Self::Status,
    ];

    pub(crate) const DETAILS: [Self; 5] = [
        Self::Project,
        Self::Id,
        Self::Filename,
        Self::Size,
        Self::Role,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Path => "path",
            Self::Source => "source",
            Self::Project => "project",
            Self::Id => "id",
            Self::Filename => "filename",
            Self::Size => "size",
            Self::Role => "role",
            Self::Status => "status",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Index => "Index",
            Self::Path => "Path",
            Self::Source => "Source",
            Self::Project => "Project ID",
            Self::Id => "File ID",
            Self::Filename => "Filename",
            Self::Size => "Size",
            Self::Role => "Role",
            Self::Status => "Status",
        }
    }

    pub(crate) fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Path => 'P',
            Self::Source => 'O',
            Self::Project => 'J',
            Self::Id => 'F',
            Self::Filename => 'A',
            Self::Size => 'B',
            Self::Role => 'R',
            Self::Status => 'S',
        }
    }
}

pub(crate) fn write_match(
    output: &mut impl Write,
    index: usize,
    path: &Path,
    source: &SourceId,
    found: Option<&FileMatch>,
    columns: &[Column],
) -> io::Result<()> {
    let view = View::new(index, path, source, found);
    if !terminal::stdout_is_tty() {
        let values: Vec<_> = columns
            .iter()
            .map(|column| tsv(&view.value(*column)))
            .collect();
        return writeln!(output, "{}", values.join("\t"));
    }
    let mut output = ConsoleWriter::new(output);
    view.write_console(&mut output, columns)?;
    writeln!(output)
}

pub(crate) fn headers(
    output: &mut impl Write,
    columns: &[Column],
    enabled: bool,
) -> io::Result<()> {
    if enabled && !terminal::stdout_is_tty() {
        writeln!(
            output,
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

/// Borrows one match for consistent standalone and nested metadata presentation.
pub(crate) struct View<'a> {
    index: usize,
    path: &'a Path,
    source: &'a SourceId,
    found: Option<&'a FileMatch>,
}

impl<'a> View<'a> {
    pub(crate) fn new(
        index: usize,
        path: &'a Path,
        source: &'a SourceId,
        found: Option<&'a FileMatch>,
    ) -> Self {
        Self {
            index,
            path,
            source,
            found,
        }
    }

    pub(crate) fn write_console(
        &self,
        output: &mut ConsoleWriter<'_>,
        columns: &[Column],
    ) -> io::Result<()> {
        let style = OutputStyle::new();
        for column in columns {
            let value = self.value(*column);
            if value.is_empty() && self.found.is_none() && columns.len() > 1 {
                continue;
            }
            let value = if value.is_empty() {
                style.muted("unavailable")
            } else {
                let value = text(&value);
                match column {
                    Column::Filename => style.primary(value),
                    Column::Source | Column::Id => style.secondary(value),
                    Column::Project => style.important(value),
                    Column::Status if self.found.is_some() => style.success(value),
                    Column::Status => style.normal(value),
                    Column::Size => style.muted(format!("{value} bytes")),
                    _ => style.muted(value),
                }
            };
            writeln!(
                output,
                "{}{value}",
                style.muted(format!("{}: ", column.label()))
            )?;
        }
        Ok(())
    }

    fn value(&self, column: Column) -> String {
        match column {
            Column::Index => self.index.to_string(),
            Column::Path => self.path.to_string_lossy().into_owned(),
            Column::Source => self.source.0.clone(),
            Column::Status => if self.found.is_some() {
                "matched"
            } else {
                "unmatched"
            }
            .into(),
            _ => self
                .found
                .map(|file| match column {
                    Column::Project => file.reference.project.id.0.clone(),
                    Column::Id => file.reference.id.0.clone(),
                    Column::Filename => file.filename.clone(),
                    Column::Size => file.size.to_string(),
                    Column::Role => match file.role {
                        MatchedFileRole::Primary => "primary",
                        MatchedFileRole::Additional => "additional",
                    }
                    .into(),
                    _ => unreachable!(),
                })
                .unwrap_or_default(),
        }
    }
}
