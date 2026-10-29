//! Startup notice rendering for builds outside the official release workflow.

use std::io::{self, Write};

use super::{console::ConsoleWriter, style::OutputStyle};

pub(crate) fn unofficial_warning(output: &mut ConsoleWriter<'_>) -> io::Result<()> {
    let style = OutputStyle::stderr();
    writeln!(
        output,
        "{} {}",
        style.warning("warning:"),
        style.normal(
            "This is not an official mcpm release build; it may be a third-party build or contain changes that have not been officially released."
        )
    )
}
