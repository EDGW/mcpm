//! Shared mod deps output for file and server commands.

use std::io::{self, Write};

use super::File;

use super::super::source_path::PathArgs;
use crate::display::{
    console::ConsoleWriter,
    dependency::{format_dependency, text},
    style::OutputStyle,
};

pub(super) fn write_deps(output: &mut impl Write, file: &File, paths: &PathArgs) -> io::Result<()> {
    let mut output = ConsoleWriter::new(output);
    let style = OutputStyle::new();
    let outer_path = text(&file.path);
    writeln!(output, "{}", style.muted(format!("File: {outer_path}")))?;
    if !file.failed_loaders.is_empty() {
        writeln!(
            output,
            "{}",
            style.warning(format!(
                "Identification: incomplete (failed loaders: {})",
                file.failed_loaders
                    .iter()
                    .map(|loader| text(loader))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        )?;
    }
    for metadata in &file.metadata {
        let records = &metadata.records;
        writeln!(
            output,
            "\n{}{}",
            style.muted("Loader: "),
            style.loader(&metadata.loader, &text(&metadata.loader_name))
        )?;
        let mut output = output.subwriter();
        for (index, record) in records.iter().enumerate() {
            if index > 0 {
                writeln!(output)?;
            }
            let info = &record.info;
            writeln!(
                output,
                "{}{} ({}) {}",
                style.muted("Mod: "),
                style.primary(text(&info.name)),
                style.important(text(&info.modid)),
                style.secondary(text(&info.version))
            )?;
            let source = paths
                .pieces_with_filename(
                    std::path::Path::new(&file.path),
                    &record.source,
                    &file.filename,
                )
                .into_iter()
                .map(|(part, paint)| style.paint(text(&part), paint))
                .collect::<String>();
            writeln!(output, "{}{source}", style.muted("Source: "))?;
            if info.dependencies.is_empty() {
                writeln!(output, "{}", style.muted("Dependencies: none declared"))?;
            } else {
                writeln!(output, "{}", style.important("Dependencies:"))?;
                let mut output = output.subwriter();
                for dependency in &info.dependencies {
                    writeln!(output, "{}", format_dependency(dependency, &style))?;
                }
            }
        }
    }
    Ok(())
}
