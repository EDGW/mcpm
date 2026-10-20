//! Shared client metadata escaping and quiet inferred-directory hints.

use std::{
    error::Error as StdError,
    io::{self, Write},
};

use crate::terminal;

pub(super) fn write_inferred_root(
    output: &mut impl Write,
    path: &std::path::Path,
    command: &str,
) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let style = crate::commands::output_style::OutputStyle::new();
    writeln!(
        output,
        "{}",
        style.muted(format!(
            "Using Minecraft root {path:?}; use `{command}` to use the current directory."
        ))
    )?;
    writeln!(output)
}

pub(super) use crate::commands::output_style::text;

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
