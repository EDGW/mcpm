//! Shared field groups and escaped TSV columns for source-identification results.

use std::{
    io::{self, Write},
    path::Path,
};

use mcpm_backend::operations::mods::identification::{QueryResult, SourceResult};
use mcpm_lib::source::{FileMatch, MatchedFileRole, SourceId};

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
    #[value(name = "query_status")]
    QueryStatus,
    #[value(name = "cache_coverage")]
    CacheCoverage,
    Origin,
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

    pub(crate) const OPTIONAL: [Self; 3] = [Self::QueryStatus, Self::CacheCoverage, Self::Origin];

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
            Self::QueryStatus => "query_status",
            Self::CacheCoverage => "cache_coverage",
            Self::Origin => "origin",
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
            Self::QueryStatus => "Query status",
            Self::CacheCoverage => "Cache coverage",
            Self::Origin => "Origin",
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
            Self::QueryStatus => 'Q',
            Self::CacheCoverage => 'C',
            Self::Origin => 'G',
        }
    }
}

/// Presents source facts alongside their shared, file-level status.
pub(crate) fn write_result(
    output: &mut impl Write,
    index: usize,
    path: &Path,
    result: &QueryResult,
    columns: &[Column],
) -> io::Result<()> {
    for source in &result.sources {
        let mut view = View::new(index, path, &source.source, source.file());
        view.identification = Some((result, source));
        write_view(output, view, columns)?;
    }
    Ok(())
}

fn write_view(output: &mut impl Write, view: View<'_>, columns: &[Column]) -> io::Result<()> {
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
    identification: Option<(&'a QueryResult, &'a SourceResult)>,
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
            identification: None,
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
            Column::Status => self
                .identification
                .map(|(_, source)| source.match_status())
                .unwrap_or(if self.found.is_some() {
                    "matched"
                } else {
                    "unmatched"
                })
                .into(),
            Column::QueryStatus => self
                .identification
                .map(|(result, _)| result.status().as_str())
                .unwrap_or_default()
                .into(),
            Column::CacheCoverage => self
                .identification
                .map(|(result, _)| result.cache_coverage().as_str())
                .unwrap_or_default()
                .into(),
            Column::Origin => self
                .identification
                .and_then(|(_, source)| source.origin())
                .unwrap_or_default()
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
