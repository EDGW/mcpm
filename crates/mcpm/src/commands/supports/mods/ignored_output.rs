//! Terminal summaries for mod files excluded by loader selection.

use std::io::{self, Write};
use std::path::PathBuf;

use crate::theme::{self, Role};

use crate::{display::console::ConsoleWriter, terminal};

pub(in crate::commands) fn write_loader_filtered(
    output: &mut impl Write,
    count: usize,
    files: &[PathBuf],
) -> io::Result<()> {
    if count == 0 || !terminal::stdout_is_tty() {
        return Ok(());
    }
    let noun = if count == 1 { "file" } else { "files" };
    let hint = format!(
        "({count} {noun} ignored due to no mod declarations recognized by the selected loaders)"
    );
    let mut output = ConsoleWriter::new(output);
    writeln!(output)?;
    let color = terminal::stdout_color_enabled();
    write_line(&mut output, &hint, color)?;
    let mut output = output.subwriter();
    for path in files {
        // Debug formatting quotes filenames and escapes terminal controls.
        let filename = path.file_name().unwrap_or(path.as_os_str());
        write_line(&mut output, &format!("{filename:?}"), color)?;
    }
    Ok(())
}

fn write_line(output: &mut ConsoleWriter<'_>, value: &str, color: bool) -> io::Result<()> {
    if color {
        writeln!(output, "{}", theme::console(Role::Muted).style(value))
    } else {
        writeln!(output, "{value}")
    }
}
