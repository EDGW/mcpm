//! Interactive search result browsing with terminal restoration on exit.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::PrintStyledContent,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use kako_mods_lib::source::{FileQuery, SearchPage, SearchQuery, Source};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Padding, Paragraph, Widget},
};
use tokio::runtime::Runtime;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::super::request;
use crate::commands::output_style::text;

pub(super) fn run(
    source: &dyn Source,
    runtime: &Runtime,
    mut query: SearchQuery,
    mut page: SearchPage,
) -> Result<()> {
    let mut screen = Screen::new().context("failed to open search results")?;
    let mut selected: usize = 0;
    let mut status = String::new();
    let mut held_page_key = None;
    loop {
        screen.render(source.id().0.as_str(), &query, &page, selected, &status)?;
        // Legacy terminals report auto-repeat as fresh presses and provide no
        // release event. A quiet interval marks the end of that repeat stream.
        if held_page_key.is_some() && !event::poll(Duration::from_millis(180))? {
            held_page_key = None;
        }
        let Event::Key(key) = event::read().context("failed to read search input")? else {
            continue;
        };
        if held_page_key == Some(key.code) {
            if key.kind == KeyEventKind::Release {
                held_page_key = None;
            }
            continue;
        }
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        held_page_key = None;
        let next_offset = match key.code {
            KeyCode::Enter => {
                if let Some(project) = page.projects.get(selected) {
                    let files_query = FileQuery {
                        game_version: query.game_version.clone(),
                        loaders: query.loader.iter().cloned().collect(),
                        ..Default::default()
                    };
                    screen.render(
                        source.id().0.as_str(),
                        &query,
                        &page,
                        selected,
                        "Loading files...",
                    )?;
                    match request::wait(
                        runtime,
                        source.list_files(&project.reference, &files_query),
                        "Fetching files...",
                        |loading| {
                            screen.render(source.id().0.as_str(), &query, &page, selected, loading)
                        },
                    ) {
                        Ok(Some(files)) => {
                            // Only one screen owns raw mode and the alternate buffer.
                            // Reopen search after the file browser has restored them.
                            drop(screen);
                            let result = super::super::files::tui::run(
                                source,
                                runtime,
                                files_query,
                                vec![(project.name.clone(), project.reference.clone(), files)],
                                None,
                            );
                            screen = Screen::new().context("failed to restore search results")?;
                            if result.as_ref().err().is_some_and(request::is_interrupted) {
                                return result;
                            }
                            status = result.err().map(|e| format!("{e:#}")).unwrap_or_default();
                            held_page_key = Some(KeyCode::Enter);
                        }
                        Ok(None) => status = "Cancelled.".into(),
                        Err(error) if request::is_interrupted(&error) => return Err(error),
                        Err(error) => status = format!("{error:#}"),
                    }
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
            KeyCode::Up | KeyCode::Char('k') if selected > 0 => {
                selected -= 1;
                None
            }
            KeyCode::Down | KeyCode::Char('j') if selected + 1 < page.projects.len() => {
                selected += 1;
                None
            }
            KeyCode::Home => {
                selected = 0;
                None
            }
            KeyCode::End => {
                selected = page.projects.len().saturating_sub(1);
                None
            }
            KeyCode::Right
            | KeyCode::PageDown
            | KeyCode::Char('n')
            | KeyCode::Down
            | KeyCode::Char('j') => {
                let next = page.offset.saturating_add(u64::from(query.limit));
                if page.projects.is_empty() || page.total.is_some_and(|total| next >= total) {
                    None
                } else {
                    Some(next)
                }
            }
            KeyCode::Left
            | KeyCode::PageUp
            | KeyCode::Char('p')
            | KeyCode::Up
            | KeyCode::Char('k') => {
                (page.offset > 0).then(|| page.offset.saturating_sub(u64::from(query.limit)))
            }
            _ => None,
        };
        if let Some(offset) = next_offset {
            held_page_key = Some(key.code);
            screen.render(
                source.id().0.as_str(),
                &query,
                &page,
                selected,
                "Loading...",
            )?;
            let previous = query.offset;
            let backwards = offset < page.offset;
            query.offset = offset;
            match request::wait(
                runtime,
                source.search(&query),
                "Searching projects...",
                |loading| screen.render(source.id().0.as_str(), &query, &page, selected, loading),
            ) {
                Ok(Some(next)) => {
                    if next.projects.is_empty() {
                        query.offset = previous;
                        status = "No more results.".into();
                        continue;
                    }
                    page = next;
                    selected = if backwards {
                        page.projects.len() - 1
                    } else {
                        0
                    };
                    status.clear();
                }
                Ok(None) => {
                    query.offset = previous;
                    status = "Cancelled.".into();
                }
                Err(error) if request::is_interrupted(&error) => return Err(error),
                Err(error) => {
                    query.offset = previous;
                    status = format!("{error:#}");
                }
            }
        }
    }
}

struct Screen {
    output: io::Stderr,
}

impl Screen {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut screen = Self {
            output: io::stderr(),
        };
        execute!(screen.output, EnterAlternateScreen, Hide)?;
        Ok(screen)
    }

    fn render(
        &mut self,
        source: &str,
        query: &SearchQuery,
        page: &SearchPage,
        selected: usize,
        status: &str,
    ) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        queue!(self.output, Clear(ClearType::All))?;
        if width < 28 || height < 12 {
            self.line(0, 0, width, "Enlarge terminal to view cards", Tone::Muted)?;
            if height > 1 {
                self.line(0, height - 1, width, "q / Esc: quit", Tone::Muted)?;
            }
            return self.output.flush();
        }
        let content_width = width.saturating_sub(4);
        self.line(2, 0, content_width, "MOD SEARCH", Tone::Theme)?;
        let query_label = if query.query.is_empty() {
            "Browse all mods"
        } else {
            &query.query
        };
        self.line(
            2,
            1,
            content_width,
            &format!("{source}  /  {query_label}"),
            Tone::Normal,
        )?;
        let filters = [
            query
                .game_version
                .as_deref()
                .map(|v| format!("Minecraft {v}")),
            query.loader.as_deref().map(str::to_owned),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ");
        let total = page
            .total
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        let range = if page.projects.is_empty() {
            "0".into()
        } else {
            format!(
                "{}–{}",
                page.offset + 1,
                page.offset + page.projects.len() as u64
            )
        };
        self.line(
            2,
            2,
            content_width,
            &format!(
                "Results {range} of {total}{}{}",
                if filters.is_empty() { "" } else { "  ·  " },
                filters
            ),
            Tone::Muted,
        )?;

        if let Some(loader) = &query.loader {
            let prefix = format!(
                "Results {range} of {total}  ·  {}",
                query
                    .game_version
                    .as_deref()
                    .map(|version| format!("Minecraft {version}  ·  "))
                    .unwrap_or_default()
            );
            let column = 2 + prefix.width().min(usize::from(content_width)) as u16;
            let value = clip(
                &text(loader),
                usize::from(content_width.saturating_sub(column - 2)),
            );
            queue!(
                self.output,
                MoveTo(column, 2),
                PrintStyledContent(
                    crate::theme::get()
                        .tui_loader(loader, crate::terminal::stderr_color_enabled())
                        .apply(value)
                )
            )?;
        }

        // Measure each header before placing cards so wrapped metadata stays inside
        // its border and selection remains visible at every terminal width.
        let available = height - 6;
        let links: Vec<_> = page
            .projects
            .iter()
            .map(|project| {
                project
                    .website_url
                    .as_ref()
                    .map(|url| wrap_link(url.as_str(), usize::from(content_width - 4)))
                    .unwrap_or_default()
            })
            .collect();
        let headers: Vec<_> = page
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| {
                project_header(
                    project,
                    usize::from(content_width - 4),
                    usize::from(available)
                        .saturating_sub(links[index].len() + 3)
                        .max(1),
                )
            })
            .collect();
        let heights: Vec<u16> = headers
            .iter()
            .zip(&links)
            .map(|(header, links)| {
                (header.len() + links.len() + 3).min(usize::from(u16::MAX)) as u16
            })
            .collect();
        let mut start = 0;
        let mut used: u32 = heights
            .iter()
            .take(selected + 1)
            .map(|h| u32::from(*h))
            .sum();
        while used > u32::from(available) && start < selected {
            used -= u32::from(heights[start]);
            start += 1;
        }
        if heights.get(selected).is_some_and(|h| *h > available) {
            self.line(
                2,
                4,
                content_width,
                "Enlarge terminal height to show the full link.",
                Tone::Warning,
            )?;
        }
        let mut top = 3;
        for index in start..page.projects.len() {
            if top + heights[index] > height - 3 {
                break;
            }
            self.card(
                (top, content_width),
                &headers[index],
                &links[index],
                page.offset + index as u64 + 1,
                &page.projects[index],
                index == selected,
            )?;
            top += heights[index];
        }
        if page.projects.is_empty() {
            self.line(
                2,
                5,
                content_width,
                "No mods found. Try different keywords or filters.",
                Tone::Muted,
            )?;
        }
        let position = if page.projects.is_empty() {
            String::new()
        } else {
            format!(
                "Selected {} / {}  ·  ↑ ↓ select / cross-page  ·  ← → page",
                selected + 1,
                page.projects.len()
            )
        };
        self.line(2, height - 3, content_width, &position, Tone::Muted)?;
        self.line(2, height - 2, content_width, status, Tone::Warning)?;
        self.line(
            2,
            height - 1,
            content_width,
            "Enter files · Esc/q quit · j/k select · n/p page",
            Tone::Muted,
        )?;
        self.output.flush()
    }

    fn card(
        &mut self,
        position: (u16, u16),
        header: &[Vec<(String, Tone)>],
        links: &[String],
        number: u64,
        project: &kako_mods_lib::source::ProjectInfo,
        selected: bool,
    ) -> io::Result<()> {
        let (top, width) = position;
        let height = (header.len() + links.len() + 3) as u16;
        let inner = width.saturating_sub(4);
        let style = |tone| {
            crate::theme::widgets::style(match tone {
                Tone::Theme => crate::theme::Role::Primary,
                Tone::Muted => crate::theme::Role::Muted,
                Tone::Normal => crate::theme::Role::Normal,
                Tone::Warning => crate::theme::Role::Warning,
            })
        };
        let mut lines: Vec<_> = header
            .iter()
            .map(|segments| {
                Line::from(
                    segments
                        .iter()
                        .map(|(value, tone)| Span::styled(value.clone(), style(*tone)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        lines.push(Line::styled(
            clip(&text(&project.summary), usize::from(inner)),
            style(Tone::Muted),
        ));
        lines.extend(
            links
                .iter()
                .map(|link| Line::styled(link.clone(), style(Tone::Muted))),
        );
        let title = if selected {
            format!(" ● {number} ")
        } else {
            format!(" {number} ")
        };
        let card = Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(style(if selected { Tone::Theme } else { Tone::Muted }))
                .padding(Padding::horizontal(1))
                .title(title),
        );
        let area = Rect::new(2, top, width, height);
        let mut buffer = Buffer::empty(area);
        card.render(area, &mut buffer);
        // Buffer diff skips continuation cells of wide glyphs so CJK metadata
        // cannot shift the rest of the card or its right border.
        CrosstermBackend::new(&mut self.output).draw(Buffer::empty(area).diff(&buffer).into_iter())
    }

    fn line(
        &mut self,
        column: u16,
        row: u16,
        width: u16,
        value: &str,
        tone: Tone,
    ) -> io::Result<()> {
        let value = clip(&text(value), usize::from(width));
        let style = crate::theme::tui(match tone {
            Tone::Theme => crate::theme::Role::Primary,
            Tone::Muted => crate::theme::Role::Muted,
            Tone::Normal => crate::theme::Role::Normal,
            Tone::Warning => crate::theme::Role::Warning,
        });
        queue!(
            self.output,
            MoveTo(column, row),
            PrintStyledContent(style.apply(value))
        )
    }
}

#[derive(Clone, Copy)]
enum Tone {
    Theme,
    Normal,
    Muted,
    Warning,
}

/// Wrap URLs by display cells, retaining every character without ellipses.
fn wrap_link(value: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let cells = ch.width().unwrap_or(0);
        if used + cells > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(ch);
        used += cells;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Pack identity fields together, wrapping long fields without losing name styling.
fn project_header(
    project: &kako_mods_lib::source::ProjectInfo,
    width: usize,
    max_lines: usize,
) -> Vec<Vec<(String, Tone)>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    let fields = [
        (text(&project.name), Tone::Theme),
        (
            project.slug.as_deref().map(text).unwrap_or_default(),
            Tone::Muted,
        ),
        (format!("ID {}", text(&project.reference.id.0)), Tone::Muted),
    ];
    for (field, tone) in fields {
        if field.is_empty() {
            continue;
        }
        if used > 0 && used + 5 + field.width() <= width {
            let value = format!("  ·  {field}");
            used += value.width();
            rows.last_mut().unwrap().push((value, tone));
        } else {
            if used > 0 {
                rows.push(Vec::new());
            }
            let lines = textwrap::wrap(&field, width);
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    rows.push(Vec::new());
                }
                used = line.width();
                rows.last_mut().unwrap().push((line.to_string(), tone));
            }
        }
    }
    if rows.len() > max_lines {
        rows.truncate(max_lines);
        // Very small terminals cannot display all identity fields in full.
        if let Some(row) = rows.last_mut() {
            let used: usize = row.iter().map(|(s, _)| s.width()).sum();
            if used < width {
                row.push(("…".into(), Tone::Muted));
            }
        }
    }
    rows
}

/// Clip by display cells, reserving one cell for an ellipsis when truncated.
fn clip(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut result: String = value
        .chars()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used < width
        })
        .collect();
    result.push('…');
    result
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(self.output, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
