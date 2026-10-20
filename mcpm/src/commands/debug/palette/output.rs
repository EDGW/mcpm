//! Console palette samples expressed and rendered with owo-colors.

use std::io::{self, Write};

use owo_colors::Style;

use crate::commands::output_style::OutputStyle;

pub(super) fn write(output: &mut impl Write) -> io::Result<()> {
    let policy = OutputStyle::new();
    writeln!(output, "mcpm palette · owo-colors · Normal / Bold / Dim")?;
    writeln!(
        output,
        "8 ordinary colors + 8 bright colors + default foreground"
    )?;
    for (expression, style) in samples() {
        writeln!(output)?;
        for (suffix, style) in [
            ("", style),
            (".bold()", style.bold()),
            (".dimmed()", style.dimmed()),
        ] {
            writeln!(
                output,
                "  {}",
                policy.paint(format!("{expression}{suffix}"), style)
            )?;
        }
    }
    output.flush()
}

fn samples() -> [(&'static str, Style); 17] {
    macro_rules! colors {
        ($($method:ident),* $(,)?) => {
            [$(
                (concat!(".", stringify!($method), "()"), Style::new().$method()),
            )*]
        };
    }
    colors![
        black,
        bright_black,
        red,
        bright_red,
        green,
        bright_green,
        yellow,
        bright_yellow,
        blue,
        bright_blue,
        magenta,
        bright_magenta,
        cyan,
        bright_cyan,
        white,
        bright_white,
        default_color,
    ]
}
