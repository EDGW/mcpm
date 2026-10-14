use std::io::{self, Write};
use std::path::PathBuf;

use owo_colors::Style;

use crate::terminal::{self, Stream};

pub(super) fn write_loader_filtered(
    output: &mut impl Write,
    count: usize,
    files: &[PathBuf],
) -> io::Result<()> {
    if count == 0 || !terminal::is_tty(Stream::Stdout) {
        return Ok(());
    }
    let noun = if count == 1 { "file" } else { "files" };
    let hint = format!(
        "({count} {noun} ignored due to no mod declarations recognized by the selected loaders)"
    );
    writeln!(output)?;
    let color = terminal::color_enabled(Stream::Stdout);
    write_line(output, &hint, color)?;
    for path in files {
        // Debug formatting quotes filenames and escapes terminal controls.
        let filename = path.file_name().unwrap_or(path.as_os_str());
        write_line(output, &format!("  {filename:?}"), color)?;
    }
    Ok(())
}

fn write_line(output: &mut impl Write, value: &str, color: bool) -> io::Result<()> {
    if color {
        writeln!(output, "{}", Style::new().bright_black().style(value))
    } else {
        writeln!(output, "{value}")
    }
}
