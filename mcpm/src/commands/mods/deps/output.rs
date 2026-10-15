use std::io::{self, Write};

use kako_mods_lib::local::mcmod::ModFile;

use super::super::dependency_output::{format_dependency, text};
use super::super::mod_records;
use super::super::output_style::OutputStyle;
use super::super::source_path::PathArgs;

pub(super) fn write_deps(
    output: &mut impl Write,
    file: &ModFile,
    failed_loaders: &[String],
    paths: &PathArgs,
) -> io::Result<()> {
    let style = OutputStyle::new();
    let outer_path = text(&file.entry.path().display().to_string());
    writeln!(output, "{}", style.secondary(format!("File: {outer_path}")))?;
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
    for metadata in &file.metadata {
        writeln!(
            output,
            "\n{}{}",
            style.secondary("Loader: "),
            style.loader(&metadata.loader.id, &text(&metadata.loader.name))
        )?;
        for (index, record) in mod_records::collect(metadata, "").into_iter().enumerate() {
            if index > 0 {
                writeln!(output)?;
            }
            let info = record.info;
            writeln!(
                output,
                "{}{} ({}) {}",
                style.secondary("  Mod: "),
                style.important(text(&info.name)),
                style.important(text(&info.modid)),
                style.important(text(&info.version))
            )?;
            let source = paths
                .parts(file.entry.path(), &record.source)
                .iter()
                .map(|part| style.secondary(text(part)))
                .collect::<Vec<_>>()
                .join(&style.important("!"));
            writeln!(output, "{}{source}", style.secondary("  Source: "))?;
            if info.dependencies.is_empty() {
                writeln!(
                    output,
                    "{}",
                    style.secondary("  Dependencies: none declared")
                )?;
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
