//! Console layouts for `Rich` text: labelled Tips, wrapped blocks and single-line prompts.
//!
//! Every layout renders spans through [`spans`], so role styling, the plain fallback and control
//! character escaping exist once. Without color, `Code` keeps its backticks and other roles
//! print their text only.

use std::io::{self, Write};

use mcpm_rich::{Block, Rich, Role, Span};

use super::{
    console::{ConsoleWriter, INDENT},
    style::{OutputStyle, text},
};

/// Indented Tip: a leading blank line, `Tip:` before the first block, wrapped blocks separated
/// by blank lines.
pub(crate) fn tip(out: &mut ConsoleWriter<'_>, tip: &Rich, style: &OutputStyle) -> io::Result<()> {
    writeln!(out)?;
    let label = style.primary("Tip:");
    layout(&mut out.subwriter(), tip, Some(&label), INDENT.len(), style)
}

/// Wrapped blocks separated by blank lines, at the writer's current indentation.
pub(crate) fn block(
    out: &mut ConsoleWriter<'_>,
    value: &Rich,
    style: &OutputStyle,
) -> io::Result<()> {
    layout(out, value, None, 0, style)
}

/// One unwrapped line for prompts and status lines; blocks and list items join with a space.
pub(crate) fn line(value: &Rich, style: &OutputStyle) -> String {
    let mut parts = Vec::new();
    for block in value.blocks() {
        match block {
            Block::Paragraph(content) => parts.push(spans(content, style)),
            Block::List(items) => parts.extend(items.iter().map(|item| spans(item, style))),
        }
    }
    parts.join(" ")
}

/// A question followed by its choices, e.g. `Continue? [y/N] `; ready for `Term::write_str`.
///
/// The default choice uses `Primary`, the others `Code`; brackets and separators are `Muted`.
pub(crate) fn prompt(question: &Rich, choices: &[Choice<'_>], style: &OutputStyle) -> String {
    let separator = style.muted("/");
    let choices = choices
        .iter()
        .map(|choice| {
            if choice.default {
                style.primary(choice.label)
            } else {
                style.code(choice.label)
            }
        })
        .collect::<Vec<_>>()
        .join(&separator);
    format!(
        "{} {}{choices}{} ",
        line(question, style),
        style.muted("["),
        style.muted("]")
    )
}

/// One answer of a [`prompt`]; at most one choice should be the default.
pub(crate) struct Choice<'a> {
    pub(crate) label: &'a str,
    pub(crate) default: bool,
}

fn layout(
    out: &mut ConsoleWriter<'_>,
    value: &Rich,
    label: Option<&str>,
    indent: usize,
    style: &OutputStyle,
) -> io::Result<()> {
    let width = textwrap::termwidth().saturating_sub(indent).max(1);
    let paragraph =
        textwrap::Options::new(width).word_splitter(textwrap::WordSplitter::NoHyphenation);
    let item = paragraph
        .clone()
        .initial_indent("- ")
        .subsequent_indent("  ");
    for (index, block) in value.blocks().iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        let label = label.filter(|_| index == 0);
        match block {
            Block::Paragraph(content) => {
                let body = spans(content, style);
                let line = match label {
                    Some(label) => format!("{label} {body}"),
                    None => body,
                };
                if content.len() == 1 && content[0].role == Role::Code {
                    // Copyable commands must not acquire hard line breaks inside arguments.
                    writeln!(out, "{line}")?;
                } else {
                    wrapped(out, &line, &paragraph)?;
                }
            }
            Block::List(items) => {
                if let Some(label) = label {
                    writeln!(out, "{label}")?;
                }
                for content in items {
                    wrapped(out, &spans(content, style), &item)?;
                }
            }
        }
    }
    Ok(())
}

fn wrapped(
    out: &mut ConsoleWriter<'_>,
    line: &str,
    options: &textwrap::Options<'_>,
) -> io::Result<()> {
    for line in textwrap::wrap(line, options) {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn spans(content: &[Span], style: &OutputStyle) -> String {
    let mut rendered = String::new();
    for span in content {
        let value = text(&span.text);
        if style.colored() {
            rendered.push_str(&style.role(span.role, value));
        } else if span.role == Role::Code {
            rendered.push('`');
            rendered.push_str(&value);
            rendered.push('`');
        } else {
            rendered.push_str(&value);
        }
    }
    rendered
}
