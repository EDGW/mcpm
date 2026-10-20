//! Shared mod deps output for file and server commands.

use std::io::{self, Write};

use kako_mods_lib::local::mcmod::ModFile;

use super::super::dependency_output::{format_dependency, text};
use super::super::mod_records::{self, RecordKey};
use super::super::source_path::PathArgs;
use crate::commands::output_style::OutputStyle;

pub(super) fn write_deps(
    output: &mut impl Write,
    file: &ModFile,
    failed_loaders: &[String],
    paths: &PathArgs,
    selected: Option<&[RecordKey]>,
) -> io::Result<()> {
    let style = OutputStyle::new();
    let outer_path = text(&file.entry.path().display().to_string());
    writeln!(output, "{}", style.muted(format!("File: {outer_path}")))?;
    if !failed_loaders.is_empty() {
        writeln!(
            output,
            "{}",
            style.warning(format!(
                "Identification: incomplete (failed loaders: {})",
                failed_loaders
                    .iter()
                    .map(|loader| text(loader))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        )?;
    }
    for (metadata_index, metadata) in file.metadata.iter().enumerate() {
        let records = mod_records::selected(metadata, metadata_index, "", selected);
        if selected.is_some() && records.is_empty() {
            continue;
        }
        writeln!(
            output,
            "\n{}{}",
            style.muted("Loader: "),
            style.loader(metadata.loader.id().as_str(), &text(metadata.loader.name()))
        )?;
        for (index, record) in records.into_iter().enumerate() {
            if index > 0 {
                writeln!(output)?;
            }
            let info = record.info;
            writeln!(
                output,
                "{}{} ({}) {}",
                style.muted("  Mod: "),
                style.primary(text(&info.name)),
                style.important(text(&info.modid)),
                style.secondary(text(&info.version))
            )?;
            let source = paths.format(file.entry.path(), &record.source, &style);
            writeln!(output, "{}{source}", style.muted("  Source: "))?;
            if info.dependencies.is_empty() {
                writeln!(output, "{}", style.muted("  Dependencies: none declared"))?;
            } else {
                writeln!(output, "{}", style.important("  Dependencies:"))?;
                for dependency in &info.dependencies {
                    writeln!(output, "    {}", format_dependency(dependency, &style))?;
                }
            }
        }
    }
    Ok(())
}
