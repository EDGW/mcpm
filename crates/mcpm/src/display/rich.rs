//! Console layouts for Rich text: tips, wrapped blocks, static widget panels and prompts.
//!
//! Plain layouts share span escaping and Console styling. Panels adapt the same semantic
//! spans to library widgets and print their rendered buffer into ordinary scrollback.

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

/// Renders standard Block/Paragraph widgets once into ordinary terminal scrollback.
/// No terminal ownership, cursor movement, raw mode, or alternate screen is involved.
pub(crate) fn panel(
    output: &mut impl Write,
    title: &str,
    value: &Rich,
    width: u16,
    style: &OutputStyle,
) -> io::Result<()> {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        text::{Line, Span as WidgetSpan},
        widgets::{Block as WidgetBlock, Padding, Paragraph, Widget, Wrap},
    };
    use unicode_width::UnicodeWidthStr;

    let roles = [
        Role::Normal,
        Role::Primary,
        Role::Code,
        Role::Secondary,
        Role::Important,
        Role::Muted,
        Role::Success,
        Role::Warning,
        Role::Danger,
    ];
    let palette = roles.map(|role| (crate::theme::widgets::style(role), role));
    let convert = |spans: &[Span]| {
        Line::from(
            spans
                .iter()
                .map(|span| {
                    WidgetSpan::styled(text(&span.text), crate::theme::widgets::style(span.role))
                })
                .collect::<Vec<_>>(),
        )
    };
    let mut lines = Vec::new();
    for block in value.blocks() {
        match block {
            Block::Paragraph(spans) => lines.push(convert(spans)),
            Block::List(items) => {
                for item in items {
                    let mut row = convert(item);
                    row.spans.insert(0, WidgetSpan::raw("- "));
                    lines.push(row);
                }
            }
        }
    }
    let width = width.max(5);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    // Measure content at the actual width after both borders and horizontal padding.
    let height = paragraph
        .line_count(width.saturating_sub(4))
        .saturating_add(2)
        .max(3)
        .min(u16::MAX as usize) as u16;
    let paragraph = paragraph.block(
        WidgetBlock::bordered()
            .title(title)
            .border_style(crate::theme::widgets::style(Role::Primary))
            .title_style(crate::theme::widgets::style(Role::Primary))
            .padding(Padding::horizontal(1)),
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    paragraph.render(buffer.area, &mut buffer);
    let paint_run = |output: &mut dyn Write,
                     key: (ratatui::style::Color, ratatui::style::Modifier),
                     run: &str|
     -> io::Result<()> {
        if style.colored()
            && let Some((_, role)) = palette.iter().find(|(candidate, _)| {
                candidate.fg == Some(key.0) && candidate.add_modifier == key.1
            })
        {
            write!(output, "{}", style.role(*role, run))
        } else {
            output.write_all(run.as_bytes())
        }
    };
    let mut run = String::with_capacity(usize::from(width));
    for row in buffer.content.chunks(usize::from(width)) {
        let mut skip = 0;
        let mut active = None;
        for cell in row {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let key = (cell.fg, cell.modifier);
            if let Some(previous) = active
                && previous != key
            {
                paint_run(output, previous, &run)?;
                run.clear();
            }
            active = Some(key);
            run.push_str(cell.symbol());
            skip = UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
        }
        if let Some(key) = active {
            paint_run(output, key, &run)?;
        }
        run.clear();
        writeln!(output)?;
    }
    output.flush()
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
                wrapped(out, &line, &paragraph)?;
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
