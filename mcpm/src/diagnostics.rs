use std::io::{self, IsTerminal};

use owo_colors::Style;

pub(crate) fn error(error: &anyhow::Error) {
    let color = io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    for (depth, cause) in error.chain().enumerate() {
        let indentation = if depth == 0 { "" } else { "  " };
        let label = if depth == 0 { "error:" } else { "caused by:" };
        if color {
            eprintln!(
                "{indentation}{} {}",
                Style::new().bright_yellow().bold().style(label),
                Style::new().bright_white().style(cause),
            );
        } else {
            eprintln!("{indentation}{label} {cause}");
        }
    }
}
