//! Client list presentation with selectable columns.

use std::io::{self, Write};

use crate::{
    display::{console::ConsoleWriter, style::OutputStyle},
    terminal,
};

use super::{Row, State};

use super::super::output::text;

pub(super) fn write_list(
    output: &mut impl Write,
    rows: &[Row],
    columns: &super::columns::ColumnArgs,
) -> io::Result<()> {
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
        let detail = text(row.detail.summary());
        if columns.is_explicit() {
            use super::columns::Column;
            let values: Vec<_> = columns
                .selected()
                .iter()
                .map(|column| match column {
                    Column::Index => (index + 1).to_string(),
                    Column::Name => name.clone(),
                    Column::State => state.to_owned(),
                    Column::Loaders => row
                        .loaders
                        .iter()
                        .map(|l| format!("{} {}", text(l.loader.name()), text(&l.version)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    Column::Detail => detail.clone(),
                })
                .collect();
            if formatted {
                let mut output = ConsoleWriter::new(output);
                let styled: Vec<_> = columns
                    .selected()
                    .iter()
                    .zip(&values)
                    .map(|(column, value)| match column {
                        Column::Name => style.primary(value),
                        Column::Loaders => row
                            .loaders
                            .iter()
                            .map(|loader| {
                                format!(
                                    "{} {}",
                                    style.loader(
                                        loader.loader.id().as_str(),
                                        &text(loader.loader.name())
                                    ),
                                    style.secondary(text(&loader.version))
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        Column::State => match row.state {
                            State::Normal => style.success(value),
                            State::Broken => style.danger(value),
                            State::Failed => style.warning(value),
                        },
                        _ => style.muted(value),
                    })
                    .collect();
                for line in textwrap::wrap(&styled.join("  "), width) {
                    writeln!(output, "{line}")?;
                }
            } else {
                writeln!(output, "{}", values.join("\t"))?;
            }
            continue;
        }
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
        let mut output = ConsoleWriter::new(output);
        writeln!(
            output,
            "{}  {}  {badge}",
            style.muted(index + 1),
            style.primary(name)
        )?;
        let mut output = output.subwriter();
        if !row.loaders.is_empty() {
            // One loader per line keeps each version legible in narrow terminals.
            for loader in &row.loaders {
                writeln!(
                    output,
                    "{}{} {}",
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
            let indentation = " ".repeat(label.len());
            let options = textwrap::Options::new(width.saturating_sub(2).max(1))
                .initial_indent(label)
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
