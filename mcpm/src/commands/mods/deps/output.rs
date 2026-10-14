use std::io::{self, Write};

use kako_mods_lib::local::mcmod::ModFile;

use super::super::dependency_output::{format_dependency, text};

pub(super) fn write_deps(
    output: &mut impl Write,
    file: &ModFile,
    failed_loaders: &[String],
) -> io::Result<()> {
    let outer_path = text(&file.entry.path().display().to_string());
    writeln!(output, "File: {outer_path}")?;
    if !failed_loaders.is_empty() {
        writeln!(
            output,
            "Identification: incomplete (failed loaders: {})",
            failed_loaders
                .iter()
                .map(|loader| text(loader))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    for metadata in &file.metadata {
        writeln!(output, "\nLoader: {}", text(&metadata.loader.name))?;
        for info in &metadata.mods {
            writeln!(
                output,
                "  Mod: {} ({}) {}",
                text(&info.name),
                text(&info.modid),
                text(&info.version)
            )?;
            let mut source = outer_path.clone();
            for member in &info.nested_path {
                source.push_str("!/");
                source.push_str(&text(member));
            }
            writeln!(output, "  Source: {source}")?;
            if info.dependencies.is_empty() {
                writeln!(output, "  Dependencies: none declared")?;
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
