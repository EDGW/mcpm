use std::{
    error::Error as StdError,
    io::{self, Write},
};

use crate::terminal::{self, Stream};

pub(super) fn write_inferred_root(
    output: &mut impl Write,
    path: &std::path::Path,
    command: &str,
) -> io::Result<()> {
    if terminal::is_raw() {
        return Ok(());
    }
    if terminal::color_enabled(Stream::Stderr) {
        let theme = owo_colors::Style::new().bright_cyan().bold();
        let white = owo_colors::Style::new().bright_white().bold();
        writeln!(
            output,
            "{}{}{}{}{}",
            theme.style("Using Minecraft root "),
            white.style(format!("{path:?}")),
            theme.style("; use `"),
            white.style(command),
            theme.style("` to use the current directory."),
        )?;
    } else {
        writeln!(
            output,
            "Using Minecraft root {path:?}; use `{command}` to use the current directory."
        )?;
    }
    writeln!(output)
}

// Escape controls so names, paths and error text cannot change terminal structure.
pub(super) fn text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() {
                ch.escape_default().to_string()
            } else {
                ch.to_string()
            }
        })
        .collect()
}

pub(super) fn error_chain(error: &dyn StdError) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        message.push_str(": ");
        message.push_str(&error.to_string());
        source = error.source();
    }
    message
}
