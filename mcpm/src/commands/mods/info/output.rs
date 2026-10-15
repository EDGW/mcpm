use std::io::{self, Write};

use super::super::dependency_output::{format_dependency, text};
use super::super::mod_records;
use super::super::output_style::OutputStyle;

use kako_mods_lib::local::mcmod::{ModFile, ModStatus};

pub(super) fn write_info(output: &mut impl Write, file: &ModFile) -> io::Result<()> {
    let style = OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.secondary(format!(
            "File: {}",
            text(&file.entry.path().display().to_string())
        ))
    )?;
    let status = match file.entry.status() {
        ModStatus::Enabled => "enabled",
        ModStatus::Disabled => "disabled",
    };
    writeln!(output, "{}", style.secondary(format!("Status: {status}")))?;

    for metadata in &file.metadata {
        writeln!(
            output,
            "\n{}{}",
            style.secondary("Loader: "),
            style.loader(&metadata.loader.id, &text(&metadata.loader.name))
        )?;
        for (index, record) in mod_records::collect(metadata, "").iter().enumerate() {
            let info = record.info;
            if index > 0 {
                writeln!(output)?;
            }
            writeln!(
                output,
                "{}{}",
                style.secondary("  Name: "),
                style.important(text(&info.name))
            )?;
            writeln!(
                output,
                "{}{}",
                style.secondary("  ID: "),
                style.important(text(&info.modid))
            )?;
            writeln!(
                output,
                "{}{}",
                style.secondary("  Version: "),
                style.important(text(&info.version))
            )?;
            if info.dependencies.is_empty() {
                writeln!(output, "{}", style.secondary("  Dependencies: none"))?;
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
