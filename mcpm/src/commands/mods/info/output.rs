use std::io::{self, Write};

use super::super::dependency_output::{format_dependency, text};

use kako_mods_lib::local::mcmod::{ModFile, ModStatus};

pub(super) fn write_info(output: &mut impl Write, file: &ModFile) -> io::Result<()> {
    writeln!(output, "File: {}", file.entry.path().display())?;
    let status = match file.entry.status() {
        ModStatus::Enabled => "enabled",
        ModStatus::Disabled => "disabled",
    };
    writeln!(output, "Status: {status}")?;

    for metadata in &file.metadata {
        writeln!(output, "\nLoader: {}", text(&metadata.loader.name))?;
        for (index, info) in metadata.mods.iter().enumerate() {
            if index > 0 {
                writeln!(output)?;
            }
            writeln!(output, "  Name: {}", text(&info.name))?;
            writeln!(output, "  ID: {}", text(&info.modid))?;
            writeln!(output, "  Version: {}", text(&info.version))?;
            if info.dependencies.is_empty() {
                writeln!(output, "  Dependencies: none")?;
            } else {
                writeln!(output, "  Dependencies:")?;
                for dependency in &info.dependencies {
                    writeln!(output, "    {}", format_dependency(dependency))?;
                }
            }
        }
    }
    Ok(())
}
