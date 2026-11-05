//! Shared client metadata escaping and quiet inferred-directory hints.

use std::io::{self, Write};

use crate::terminal;

pub(super) fn write_inferred_root(
    output: &mut impl Write,
    path: &std::path::Path,
    command: &str,
) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let style = crate::display::style::OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.muted(format!(
            "Using Minecraft root {path:?}; use `{command}` to use the current directory."
        ))
    )?;
    writeln!(output)
}

pub(super) use crate::display::style::text;
