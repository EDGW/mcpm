//! Client list presentation using semantic console styles.

use std::io::{self, Write};

use crate::{commands::output_style::OutputStyle, terminal};

use super::{Row, State};

use super::super::output::text;

pub(super) fn write_list(output: &mut impl Write, rows: &[Row]) -> io::Result<()> {
    let formatted = terminal::stdout_is_tty();
    let style = OutputStyle::new();
    let width = textwrap::termwidth().max(1);
    for (index, row) in rows.iter().enumerate() {
        let state = match row.state {
            State::Normal => "normal",
            State::Broken => "broken",
            State::Failed => "failed",
        };
        let name = text(&row.name);
        let detail = text(&row.detail);
        if !formatted {
            let loaders = row
                .loaders
                .iter()
                .map(|loader| format!("{} {}", text(loader.loader.name()), text(&loader.version)))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                output,
                "{}\t{name}\t{state}\t{loaders}\t{detail}",
                index + 1
            )?;
            continue;
        }
        let badge = format!("[{state}]");
        let badge = match row.state {
            State::Normal => style.success(badge),
            State::Broken => style.danger(badge),
            State::Failed => style.warning(badge),
        };
        writeln!(
            output,
            "{}  {}  {badge}",
            style.muted(index + 1),
            style.primary(name)
        )?;
        if !row.loaders.is_empty() {
            // One loader per line keeps each version legible in narrow terminals.
            for loader in &row.loaders {
                writeln!(
                    output,
                    "   {}{} {}",
                    style.muted("Loader: "),
                    style.loader(loader.loader.id().as_str(), &text(loader.loader.name())),
                    style.secondary(text(&loader.version))
                )?;
            }
        } else {
            let label = match row.state {
                State::Normal => "Loader: ",
                State::Broken => "Reason: ",
                State::Failed => "Error: ",
            };
            let prefix = format!("   {label}");
            let indentation = " ".repeat(prefix.len());
            let options = textwrap::Options::new(width)
                .initial_indent(&prefix)
                .subsequent_indent(&indentation)
                .word_splitter(textwrap::WordSplitter::NoHyphenation);
            for line in textwrap::wrap(&detail, options) {
                writeln!(
                    output,
                    "{}",
                    match row.state {
                        State::Normal => style.loader("vanilla", &line),
                        _ => style.warning(line),
                    }
                )?;
            }
        }
    }
    Ok(())
}
