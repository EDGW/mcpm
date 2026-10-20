//! Terminal rows and escaped TSV output for matched and unmatched local files.

use std::{
    io::{self, Write},
    path::Path,
};

use kako_mods_lib::source::{FileMatch, MatchedFileRole, SourceId};

use super::columns::Column;
use crate::{
    commands::output_style::{OutputStyle, text},
    terminal,
};

pub(super) fn write_match(
    output: &mut impl Write,
    index: usize,
    path: &Path,
    source: &SourceId,
    found: Option<&FileMatch>,
    columns: &[Column],
) -> io::Result<()> {
    let values: Vec<_> = columns
        .iter()
        .map(|column| {
            text(&match column {
                Column::Index => index.to_string(),
                Column::Path => path.to_string_lossy().into_owned(),
                Column::Source => source.0.clone(),
                Column::Status => if found.is_some() {
                    "matched"
                } else {
                    "unmatched"
                }
                .into(),
                _ => found
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
            })
        })
        .collect();
    if !terminal::stdout_is_tty() {
        return writeln!(output, "{}", values.join("\t"));
    }
    let style = OutputStyle::new();
    let width = textwrap::termwidth().saturating_sub(2).max(1);
    let values: Vec<_> = columns
        .iter()
        .zip(values)
        .map(|(column, value)| match column {
            Column::Status if found.is_none() => style.warning(value),
            Column::Status => style.success(value),
            Column::Project => style.important(value),
            _ => style.muted(value),
        })
        .collect();
    for line in textwrap::wrap(&values.join("  "), width) {
        writeln!(output, "{line}")?;
    }
    Ok(())
}
