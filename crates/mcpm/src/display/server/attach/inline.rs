//! Ratatui inline viewport and standard widgets; never enters the alternate screen.

use crate::{
    theme::{Role, widgets::style},
    widgets::browser::panel,
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::{Backend, CrosstermBackend},
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Widget, Wrap},
};
use std::{
    collections::VecDeque,
    io::{self, Write},
};
use unicode_width::UnicodeWidthStr;

pub(super) struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    height: u16,
    suggestion_rows: u16,
    raw: bool,
    output: VecDeque<Vec<ratatui::buffer::Cell>>,
}
impl Screen {
    pub fn new(height: u16) -> io::Result<Self> {
        let mut backend = CrosstermBackend::new(io::stdout());
        let size = backend.size()?;
        let position = backend.get_cursor_position()?;
        let height = height.min(size.height).max(1);
        let top = size.height.saturating_sub(height);
        if position.y < top {
            backend.append_lines(top - position.y)?;
        }
        let mut screen = Self {
            terminal: Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(height),
                },
            )?,
            height,
            suggestion_rows: 0,
            raw: false,
            output: VecDeque::new(),
        };
        screen.terminal.clear()?;
        crossterm::terminal::enable_raw_mode()?;
        screen.raw = true;
        Ok(screen)
    }
    /// Caller pauses the keyboard reader while Ratatui queries the cursor position.
    pub fn resize(&mut self, grow: bool) -> io::Result<()> {
        let size = self.terminal.size()?;
        let target = if grow {
            self.height.saturating_add(1)
        } else {
            self.height.saturating_sub(1)
        }
        .max(3)
        .min(size.height.max(1));
        self.height = target;
        self.resize_viewport(
            target
                .saturating_add(self.suggestion_rows)
                .min(size.height.max(1)),
        )
    }
    fn resize_viewport(&mut self, target: u16) -> io::Result<()> {
        let size = self.terminal.size()?;
        if target == self.terminal.get_frame().area().height {
            return Ok(());
        }
        let previous = self.terminal.get_frame().area();
        self.terminal.clear()?;
        let top = size.height.saturating_sub(target);
        // Do not append/scroll to allocate a larger viewport: that irreversibly moves output
        // into terminal scrollback. Repaint the bounded visible tail when either edge moves.
        let visible = self.output.len().min(top as usize) as u16;
        let start = top - visible;
        let old_start = previous
            .y
            .saturating_sub(self.output.len().min(previous.y as usize) as u16);
        let mut restored = Buffer::empty(Rect::new(0, start, size.width, visible));
        for (offset, row) in self.output.iter().rev().take(top as usize).enumerate() {
            let y = top as usize - offset - 1;
            for (x, cell) in row.iter().take(size.width as usize).enumerate() {
                restored[(x as u16, y as u16)] = cell.clone();
            }
        }
        for y in old_start.min(start)..top {
            self.terminal.backend_mut().set_cursor_position((0, y))?;
            self.terminal
                .backend_mut()
                .clear_region(ratatui::backend::ClearType::UntilNewLine)?;
        }
        self.terminal
            .backend_mut()
            .draw(Buffer::empty(restored.area).diff(&restored).into_iter())?;
        self.terminal.backend_mut().set_cursor_position((0, top))?;
        self.terminal = Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(target),
            },
        )?;
        // Growing the viewport claims rows previously occupied by logs. A fresh
        // Terminal assumes blank cells, so explicitly clear the newly owned area.
        self.terminal.clear()?;
        Ok(())
    }
    pub fn line(&mut self, line: Line<'_>) -> io::Result<()> {
        self.terminal.autoresize()?;
        let width = self.terminal.size()?.width.max(1);
        let paragraph = Paragraph::new(line).wrap(Wrap { trim: false });
        let height = paragraph.line_count(width).max(1).min(u16::MAX as usize) as u16;
        let limit = self.terminal.size()?.height as usize;
        let output = &mut self.output;
        self.terminal.insert_before(height, |buffer| {
            paragraph.render(buffer.area, buffer);
            remember(output, buffer, limit);
        })
    }
    pub fn divider(&mut self) -> io::Result<()> {
        let limit = self.terminal.size()?.height as usize;
        let output = &mut self.output;
        self.terminal.insert_before(1, |buffer| {
            Block::new()
                .borders(Borders::TOP)
                .border_style(style(Role::Muted))
                .title(" History ")
                .render(buffer.area, buffer);
            remember(output, buffer, limit);
        })
    }
    pub fn rcon(&mut self, name: &str, text: &str, first: bool, last: bool) -> io::Result<()> {
        self.terminal.autoresize()?;
        let width = self.terminal.size()?.width.max(1);
        let (widget, height) = response(name, text, width, first, last);
        let limit = self.terminal.size()?.height as usize;
        let output = &mut self.output;
        self.terminal.insert_before(height, |buffer| {
            widget.render(buffer.area, buffer);
            remember(output, buffer, limit);
        })
    }
    pub fn draw(
        &mut self,
        draft: &str,
        cursor: usize,
        status: &str,
        partial: &str,
        suggestions: super::suggestions::View<'_>,
    ) -> io::Result<()> {
        self.terminal.autoresize()?;
        let size = self.terminal.size()?;
        let available = size.height.saturating_sub(self.height.min(size.height));
        let content_width = size.width.saturating_sub(4);
        let mut row_height = 0usize;
        let mut selected_start = 0usize;
        let mut selected_end = 0usize;
        for (index, line) in suggestions.rows.iter().enumerate() {
            let height = Paragraph::new(line.clone())
                .wrap(Wrap { trim: false })
                .line_count(content_width);
            if suggestions.selected == Some(index) {
                selected_start = row_height;
                selected_end = row_height.saturating_add(height);
            }
            row_height = row_height.saturating_add(height);
        }
        self.suggestion_rows = if row_height == 0 || available < 2 {
            0
        } else {
            row_height.saturating_add(1).min(available as usize) as u16
        };
        let scroll = selected_end
            .saturating_sub(self.suggestion_rows.saturating_sub(1) as usize)
            .min(selected_start)
            .min(u16::MAX as usize) as u16;
        self.resize_viewport(
            self.height
                .saturating_add(self.suggestion_rows)
                .min(size.height.max(1)),
        )?;
        self.terminal.draw(|frame| {
            let [hints, input, footer] = Layout::vertical([
                Constraint::Length(self.suggestion_rows),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .areas(frame.area());
            let hint_frame = Block::new()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(style(Role::Muted))
                .title(Line::styled(suggestions.title, style(Role::Muted)))
                .padding(Padding::horizontal(1));
            frame.render_widget(
                Paragraph::new(suggestions.rows)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0))
                    .block(hint_frame),
                hints,
            );
            let width = input.width.saturating_sub(4) as usize;
            let cursor_width = UnicodeWidthStr::width(&draft[..cursor]);
            let horizontal = cursor_width.saturating_sub(width.saturating_sub(1));
            frame.render_widget(
                Paragraph::new(draft)
                    .block(
                        panel(
                            format!(" Command Input · {}+↑/↓ height ", modifier_name()),
                            true,
                        )
                        .padding(Padding::horizontal(1)),
                    )
                    .scroll((0, horizontal.min(u16::MAX as usize) as u16)),
                input,
            );
            frame.render_widget(
                Paragraph::new(if partial.is_empty() { status } else { partial })
                    .style(style(Role::Muted)),
                footer,
            );
            if input.width > 4 && input.height > 1 {
                frame.set_cursor_position((
                    input.x
                        + 2
                        + cursor_width
                            .saturating_sub(horizontal)
                            .min(width.saturating_sub(1)) as u16,
                    input.y + 1,
                ));
            }
        })?;
        Ok(())
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.terminal.clear();
        let area = self.terminal.get_frame().area();
        let _ = self.terminal.backend_mut().set_cursor_position((0, area.y));
        let _ = self.terminal.show_cursor();
        if self.raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

fn response<'a>(
    name: &str,
    text: &'a str,
    width: u16,
    first: bool,
    last: bool,
) -> (Paragraph<'a>, u16) {
    let mut borders = Borders::LEFT | Borders::RIGHT;
    if first {
        borders |= Borders::TOP;
    }
    if last {
        borders |= Borders::BOTTOM;
    }
    let mut block = Block::new()
        .borders(borders)
        .border_style(style(Role::Primary))
        .title_style(style(Role::Primary));
    if first {
        block = block.title(Line::from(vec![
            Span::styled(
                " RCON: ",
                style(Role::Primary).remove_modifier(Modifier::BOLD),
            ),
            Span::styled(
                name.to_owned(),
                style(Role::Primary).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    }
    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    let height = paragraph
        .line_count(width)
        .max(usize::from(first) + usize::from(last) + 1)
        .min(u16::MAX as usize) as u16;
    (paragraph, height)
}

/// Console uses the same standard Block/Paragraph widgets without opening an interactive terminal.
pub(super) fn console_response(
    output: &mut impl Write,
    name: &str,
    text: &str,
    width: u16,
    color: bool,
    first: bool,
    last: bool,
) -> io::Result<()> {
    let (widget, height) = response(name, text, width.max(3), first, last);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(3), height));
    widget.render(buffer.area, &mut buffer);
    write_buffer(output, &buffer, color)
}

pub(super) fn console_divider(output: &mut impl Write, width: u16) -> io::Result<()> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(1), 1));
    Block::new()
        .borders(Borders::TOP)
        .title(" History ")
        .border_style(style(Role::Muted))
        .title_style(style(Role::Muted))
        .render(buffer.area, &mut buffer);
    write_buffer(output, &buffer, crate::terminal::stdout_color_enabled())
}

fn write_buffer(output: &mut impl Write, buffer: &Buffer, color: bool) -> io::Result<()> {
    for row in buffer.content.chunks(buffer.area.width.max(1) as usize) {
        let mut skip = 0;
        for cell in row {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let role = [Role::Primary, Role::Muted]
                .into_iter()
                .find(|role| style(*role).fg == Some(cell.fg));
            if color && let Some(role) = role {
                write!(
                    output,
                    "{}",
                    if cell.modifier.contains(Modifier::BOLD) {
                        crate::theme::console(role)
                    } else {
                        crate::theme::console(role).remove_effect(owo_colors::Effect::Bold)
                    }
                    .style(cell.symbol())
                )?;
            } else {
                output.write_all(cell.symbol().as_bytes())?;
            }
            skip = UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
        }
        writeln!(output)?;
    }
    Ok(())
}

pub(crate) const fn modifier_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Option"
    } else {
        "Alt"
    }
}

fn remember(output: &mut VecDeque<Vec<ratatui::buffer::Cell>>, buffer: &Buffer, limit: usize) {
    for row in buffer.content.chunks(buffer.area.width.max(1) as usize) {
        output.push_back(row.to_vec());
        while output.len() > limit {
            output.pop_front();
        }
    }
}
