//! Ratatui inline viewport and standard widgets; never enters the alternate screen.

use crate::{theme::widgets::style, widgets::browser::panel};
use mcpm_rich::Role;
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
    speed_rows: u16,
    raw: bool,
    output: VecDeque<Vec<ratatui::buffer::Cell>>,
    // Rows before this boundary belong to output printed before this attachment.
    history_end: u16,
}
impl Screen {
    pub fn new(height: u16, speed: Vec<Line<'static>>) -> io::Result<Self> {
        let mut backend = CrosstermBackend::new(io::stdout());
        let size = backend.size()?;
        let position = backend.get_cursor_position()?;
        let height = height.min(size.height).max(1);
        let speed_rows = Paragraph::new(speed)
            .wrap(Wrap { trim: false })
            .line_count(size.width)
            .min(size.height.saturating_sub(height) as usize) as u16;
        let viewport_height = height.saturating_add(speed_rows);
        let top = size.height.saturating_sub(viewport_height);
        if position.y < top {
            backend.append_lines(top - position.y)?;
        }
        let mut screen = Self {
            terminal: Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(viewport_height),
                },
            )?,
            height,
            suggestion_rows: 0,
            speed_rows,
            raw: false,
            output: VecDeque::new(),
            history_end: top,
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
                .saturating_add(self.speed_rows)
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
        let visible = self.output.len().min(top as usize) as u16;
        let start = top - visible;
        self.history_end = self.history_end.min(previous.y);
        // The cache owns only our output. Preserve unknown terminal rows by scrolling
        // them upward before a larger viewport or replay would occupy their cells.
        let preserve = self.history_end.saturating_sub(start);
        if preserve > 0 {
            self.terminal
                .backend_mut()
                .set_cursor_position((0, size.height.saturating_sub(1)))?;
            self.terminal.backend_mut().append_lines(preserve)?;
            self.history_end -= preserve;
        }
        let mut restored = Buffer::empty(Rect::new(0, start, size.width, visible));
        for (offset, row) in self.output.iter().rev().take(top as usize).enumerate() {
            let y = top as usize - offset - 1;
            for (x, cell) in row.iter().take(size.width as usize).enumerate() {
                restored[(x as u16, y as u16)] = cell.clone();
            }
        }
        for y in self.history_end..top {
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
    fn insert_before(
        &mut self,
        height: u16,
        render: impl FnOnce(&mut Buffer),
    ) -> io::Result<()> {
        let limit = self.terminal.size()?.height as usize;
        let output = &mut self.output;
        let previous_top = self.terminal.get_frame().area().y;
        self.terminal.insert_before(height, |buffer| {
            render(buffer);
            remember(output, buffer, limit);
        })?;
        self.scrolled_output(height, previous_top);
        Ok(())
    }
    pub fn line(&mut self, line: Line<'_>) -> io::Result<()> {
        self.terminal.autoresize()?;
        let width = self.terminal.size()?.width.max(1);
        let paragraph = Paragraph::new(line).wrap(Wrap { trim: false });
        let height = paragraph.line_count(width).max(1).min(u16::MAX as usize) as u16;
        self.insert_before(height, |buffer| paragraph.render(buffer.area, buffer))
    }
    pub fn divider(&mut self) -> io::Result<()> {
        self.insert_before(1, |buffer| {
            Block::new()
                .borders(Borders::TOP)
                .border_style(style(Role::Muted))
                .title(" History ")
                .render(buffer.area, buffer);
        })
    }
    pub fn rcon(
        &mut self,
        label: &str,
        name: &str,
        origin: &str,
        text: &str,
        first: bool,
        last: bool,
    ) -> io::Result<()> {
        self.terminal.autoresize()?;
        let width = self.terminal.size()?.width.max(1);
        let (widget, height, _) = response(label, name, origin, text, width, first, last);
        self.insert_before(height, |buffer| widget.render(buffer.area, buffer))
    }
    pub fn exit_report(
        &mut self,
        title: &str,
        role: Role,
        lines: Vec<Line<'_>>,
    ) -> io::Result<()> {
        self.terminal.autoresize()?;
        let width = self.terminal.size()?.width.max(1);
        let (widget, height) = exit_report_widget(title, role, lines, width);
        self.insert_before(height, |buffer| widget.render(buffer.area, buffer))
    }

    fn scrolled_output(&mut self, lines: u16, previous_top: u16) {
        let advanced = self
            .terminal
            .get_frame()
            .area()
            .y
            .saturating_sub(previous_top);
        self.history_end = self
            .history_end
            .saturating_sub(lines.saturating_sub(advanced));
    }
    pub fn draw(
        &mut self,
        draft: &str,
        cursor: usize,
        status: &str,
        partial: &str,
        suggestions: super::suggestions::View<'_>,
        speed: Vec<Line<'static>>,
    ) -> io::Result<()> {
        self.terminal.autoresize()?;
        let size = self.terminal.size()?;
        let speed = Paragraph::new(speed)
            .style(style(Role::Muted))
            .wrap(Wrap { trim: false });
        self.speed_rows = speed
            .line_count(size.width)
            .min(size.height.saturating_sub(self.height) as usize) as u16;
        let available = size
            .height
            .saturating_sub(self.height)
            .saturating_sub(self.speed_rows);
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
                .saturating_add(self.speed_rows)
                .min(size.height.max(1)),
        )?;
        let leading_spaces = draft.len() - draft.trim_start_matches(' ').len();
        let diagnostic = suggestions.diagnostic.as_ref();
        let leading_warning = "Leading spaces: Minecraft may not recognize this command";
        let title = if let Some(diagnostic) = diagnostic {
            let prefix = if leading_spaces > 0 {
                format!(" · {leading_warning}")
            } else {
                String::new()
            };
            Line::styled(
                format!(
                    " Command Input · {} · Enter still sends{prefix} ",
                    diagnostic.message
                ),
                style(Role::Danger),
            )
        } else if leading_spaces > 0 {
            Line::styled(
                format!(" Command Input · {leading_warning} "),
                style(Role::Warning),
            )
        } else {
            Line::from(format!(" Command Input · {}+↑/↓ height ", modifier_name()))
        };
        let mut spans = Vec::new();
        if leading_spaces > 0 {
            // Reverse colors make otherwise invisible space cells visible without changing text.
            spans.push(Span::styled(
                &draft[..leading_spaces],
                style(Role::Warning).add_modifier(Modifier::REVERSED),
            ));
        }
        if let Some(diagnostic) = diagnostic {
            spans.push(Span::raw(&draft[leading_spaces..diagnostic.range.start]));
            spans.push(Span::styled(
                &draft[diagnostic.range.clone()],
                style(Role::Danger).add_modifier(Modifier::UNDERLINED),
            ));
            spans.push(Span::raw(&draft[diagnostic.range.end..]));
        } else {
            spans.push(Span::raw(&draft[leading_spaces..]));
        }
        let content = Line::from(spans);
        self.terminal.draw(|frame| {
            let [hints, input, footer, traffic] = Layout::vertical([
                Constraint::Length(self.suggestion_rows),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(self.speed_rows),
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
                Paragraph::new(content)
                    .block(panel(title, true).padding(Padding::horizontal(1)))
                    .scroll((0, horizontal.min(u16::MAX as usize) as u16)),
                input,
            );
            frame.render_widget(
                Paragraph::new(if partial.is_empty() { status } else { partial })
                    .style(style(Role::Muted)),
                footer,
            );
            frame.render_widget(speed, traffic);
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
    label: &str,
    name: &str,
    origin: &str,
    text: &'a str,
    width: u16,
    first: bool,
    last: bool,
) -> (Paragraph<'a>, u16, Role) {
    let mut borders = Borders::LEFT | Borders::RIGHT;
    if first {
        borders |= Borders::TOP;
    }
    if last {
        borders |= Borders::BOTTOM;
    }
    let role = if origin.is_empty() {
        Role::Primary
    } else {
        Role::Success
    };
    let mut block = Block::new()
        .borders(borders)
        .border_style(style(role))
        .title_style(style(role));
    if first {
        block = block.title(Line::from(vec![
            Span::styled(
                format!(" {label}: "),
                style(role).remove_modifier(Modifier::BOLD),
            ),
            Span::styled(name.to_owned(), style(role).add_modifier(Modifier::BOLD)),
            Span::styled(
                if origin.is_empty() {
                    " ".to_owned()
                } else {
                    format!(" · {origin} ")
                },
                style(role).remove_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    let height = paragraph
        .line_count(width)
        .max(usize::from(first) + usize::from(last) + 1)
        .min(u16::MAX as usize) as u16;
    (paragraph, height, role)
}

fn exit_report_widget<'a>(
    title: &str,
    role: Role,
    lines: Vec<Line<'a>>,
    width: u16,
) -> (Paragraph<'a>, u16) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(style(role))
        .title_style(style(role))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(" Exit: ", style(role).remove_modifier(Modifier::BOLD)),
            Span::styled(title.to_owned(), style(role).add_modifier(Modifier::BOLD)),
            Span::styled(" ", style(role).remove_modifier(Modifier::BOLD)),
        ]));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    let height = paragraph.line_count(width).max(3).min(u16::MAX as usize) as u16;
    (paragraph, height)
}

/// Console uses the same standard Block/Paragraph widgets without opening an interactive terminal.
pub(super) fn console_response(
    output: &mut impl Write,
    label: &str,
    name: &str,
    origin: &str,
    text: &str,
    width: u16,
    color: bool,
    first: bool,
    last: bool,
) -> io::Result<()> {
    let (widget, height, role) = response(label, name, origin, text, width.max(3), first, last);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(3), height));
    widget.render(buffer.area, &mut buffer);
    write_buffer(output, &buffer, color, &[role, Role::Muted])
}

/// Renders the exit report card with the same Block/Paragraph widgets as inline TUI.
pub(super) fn console_exit_report(
    output: &mut impl Write,
    title: &str,
    role: Role,
    lines: Vec<Line<'_>>,
    width: u16,
    color: bool,
) -> io::Result<()> {
    let (widget, height) = exit_report_widget(title, role, lines, width.max(5));
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(5), height));
    widget.render(buffer.area, &mut buffer);
    write_buffer(output, &buffer, color, &[role, Role::Muted])
}

pub(super) fn console_divider(output: &mut impl Write, width: u16) -> io::Result<()> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width.max(1), 1));
    Block::new()
        .borders(Borders::TOP)
        .title(" History ")
        .border_style(style(Role::Muted))
        .title_style(style(Role::Muted))
        .render(buffer.area, &mut buffer);
    write_buffer(
        output,
        &buffer,
        crate::terminal::stdout_color_enabled(),
        &[Role::Muted],
    )
}

fn write_buffer(
    output: &mut impl Write,
    buffer: &Buffer,
    color: bool,
    roles: &[Role],
) -> io::Result<()> {
    for row in buffer.content.chunks(buffer.area.width.max(1) as usize) {
        let mut skip = 0;
        for cell in row {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let role = if cell.fg == ratatui::style::Color::Reset {
                None
            } else {
                roles
                    .iter()
                    .copied()
                    .find(|role| style(*role).fg == Some(cell.fg))
            };
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
