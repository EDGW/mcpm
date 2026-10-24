//! Aligned terminal project lists and plain TSV output with selectable columns.

use std::io::{self, Write};

use kako_mods_lib::source::SearchPage;
use unicode_width::UnicodeWidthStr;

use super::columns::Column;
use crate::{
    display::style::{OutputStyle, text},
    terminal,
};

pub(super) fn write_page(
    output: &mut impl Write,
    page: &SearchPage,
    columns: &[Column],
) -> io::Result<()> {
    let rows: Vec<Vec<String>> = page
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            columns
                .iter()
                .map(|column| {
                    text(&match column {
                        Column::Index => (page.offset + index as u64 + 1).to_string(),
                        Column::Source => project.reference.source.0.clone(),
                        Column::Name => project.name.clone(),
                        Column::Slug => project.slug.clone().unwrap_or_default(),
                        Column::Id => project.reference.id.0.clone(),
                        Column::Summary => project.summary.clone(),
                        Column::Url => project
                            .website_url
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .collect();
    if !terminal::stdout_is_tty() {
        for row in rows {
            writeln!(output, "{}", row.join("\t"))?;
        }
        return Ok(());
    }
    let style = OutputStyle::new();
    let mut widths: Vec<usize> = (0..columns.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].width())
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let available = textwrap::termwidth().saturating_sub(1);
    let gaps = columns.len().saturating_sub(1) * 2;
    // A very narrow terminal stacks cells, preserving every field in full.
    if available < columns.len() + gaps {
        for row in &rows {
            for (column, value) in columns.iter().zip(row) {
                for line in textwrap::wrap(value, available.max(1)) {
                    writeln!(output, "{}", paint(&style, *column, &line))?;
                }
            }
            writeln!(output)?;
        }
        return Ok(());
    }
    while widths.iter().sum::<usize>() + gaps > available {
        let widest = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, width)| *width)
            .unwrap()
            .0;
        widths[widest] -= 1;
    }
    for row in &rows {
        let cells: Vec<_> = row
            .iter()
            .zip(&widths)
            .map(|(value, width)| textwrap::wrap(value, *width))
            .collect();
        let lines = cells.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..lines {
            for (index, column) in columns.iter().enumerate() {
                let value = cells[index].get(line).map(|s| s.as_ref()).unwrap_or("");
                write!(output, "{}", paint(&style, *column, value))?;
                if index + 1 < columns.len() {
                    write!(
                        output,
                        "{}",
                        " ".repeat(widths[index].saturating_sub(value.width()) + 2)
                    )?;
                }
            }
            writeln!(output)?;
        }
    }
    Ok(())
}

fn paint(style: &OutputStyle, column: Column, value: &str) -> String {
    match column {
        Column::Name => style.primary(value),
        Column::Summary => style.muted(value),
        _ => style.muted(value),
    }
}
