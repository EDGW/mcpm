use owo_colors::Style;

use crate::terminal;

pub(crate) fn error(error: &anyhow::Error) {
    let color = terminal::color_enabled();
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
