//! Shared mod info output for file and server commands.

use std::io::{self, Write};

use super::super::dependency_output::{format_dependency, text};
use super::super::mod_records::{self, RecordKey};
use super::super::source_path;
use crate::commands::output_style::OutputStyle;

use kako_mods_lib::local::mcmod::{ModFile, ModStatus};

pub(super) fn write_info(
    output: &mut impl Write,
    file: &ModFile,
    selected: Option<&[RecordKey]>,
) -> io::Result<()> {
    let style = OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.muted(format!(
            "File: {}",
            text(&file.entry.path().display().to_string())
        ))
    )?;
    let status = match file.entry.status() {
        ModStatus::Enabled => "enabled",
        ModStatus::Disabled => "disabled",
    };
    writeln!(output, "{}", style.muted(format!("Status: {status}")))?;

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
        for (index, record) in records.iter().enumerate() {
            let info = record.info;
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
                source_path::full(file.entry.path(), &record.source, &style)
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
    }
    Ok(())
}
