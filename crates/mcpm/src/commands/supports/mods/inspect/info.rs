//! Shared mod info output for file and server commands.

use std::io::{self, Write};

use crate::display::{
    console::ConsoleWriter,
    style::{OutputStyle, text},
};

use super::{File, model::Metadata};

pub(super) fn write_info(output: &mut impl Write, file: &File) -> io::Result<()> {
    let mut output = ConsoleWriter::new(output);
    let style = OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.muted(format!("File: {}", text(&file.path)))
    )?;
    let status = if file.enabled { "enabled" } else { "disabled" };
    writeln!(output, "{}", style.muted(format!("Status: {status}")))?;

    for metadata in &file.metadata {
        write_metadata(&mut output, std::path::Path::new(&file.path), metadata)?;
    }
    Ok(())
}

/// Renders one loader's declarations, including dependencies and nested sources.
fn write_metadata(
    output: &mut ConsoleWriter<'_>,
    path: &std::path::Path,
    metadata: &Metadata,
) -> io::Result<()> {
    crate::display::mod_meta::write(
        output,
        path,
        crate::display::mod_meta::View::declarations(
            &metadata.loader,
            &metadata.loader_name,
            metadata
                .records
                .iter()
                .map(|item| crate::display::mod_meta::Declaration {
                    info: &item.info,
                    source: item.source.iter().map(String::as_str).collect(),
                }),
        ),
    )
}
