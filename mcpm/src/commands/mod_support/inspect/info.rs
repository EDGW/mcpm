//! Shared mod info output for file and server commands.

use std::io::{self, Write};

use super::super::dependency_output::{format_dependency, text};
use super::super::source_path;
use crate::commands::output_style::OutputStyle;

use super::{File, model::Metadata};

pub(super) fn write_info(output: &mut impl Write, file: &File) -> io::Result<()> {
    let style = OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.muted(format!("File: {}", text(&file.path)))
    )?;
    let status = if file.enabled { "enabled" } else { "disabled" };
    writeln!(output, "{}", style.muted(format!("Status: {status}")))?;

    for metadata in &file.metadata {
        write_metadata(output, std::path::Path::new(&file.path), metadata)?;
    }
    Ok(())
}

/// Renders one loader's declarations, including dependencies and nested sources.
pub(in crate::commands) fn write_metadata(
    output: &mut impl Write,
    path: &std::path::Path,
    metadata: &Metadata,
) -> io::Result<()> {
    let style = OutputStyle::new();
    let records = &metadata.records;
    writeln!(
        output,
        "\n{}{}",
        style.muted("Loader: "),
        style.loader(&metadata.loader, &text(&metadata.loader_name))
    )?;
    for (index, record) in records.iter().enumerate() {
        let info = &record.info;
        if index > 0 {
            writeln!(output)?;
        }
        writeln!(
            output,
            "{}{}",
            style.muted("  Name: "),
            style.primary(text(&info.name))
        )?;
        writeln!(
            output,
            "{}{}",
            style.muted("  ID: "),
            style.important(text(&info.modid))
        )?;
        writeln!(
            output,
            "{}{}",
            style.muted("  Version: "),
            style.secondary(text(&info.version))
        )?;
        writeln!(
            output,
            "{}{}",
            style.muted("  Source: "),
            source_path::full(path, &record.source, &style)
        )?;
        if info.dependencies.is_empty() {
            writeln!(output, "{}", style.muted("  Dependencies: none"))?;
        } else {
            writeln!(output, "{}", style.important("  Dependencies:"))?;
            for dependency in &info.dependencies {
                writeln!(output, "    {}", format_dependency(dependency, &style))?;
            }
        }
    }
    Ok(())
}
