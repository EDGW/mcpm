//! Interactive file cards, project switching, and release pagination.

use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use kako_mods_lib::source::{
    FilePage, FileQuery, ProjectFile, ProjectRef, ReleaseType, Source,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
};
use tokio::runtime::Runtime;

use super::super::request;
use crate::{
    commands::output_style::text,
    theme::{Role, widgets},
};

pub(in crate::commands::source_support) fn run(
    source: &dyn Source,
    runtime: &Runtime,
    query: FileQuery,
    projects: Vec<(String, ProjectRef, FilePage)>,
    selected_file: Option<ProjectFile>,
) -> Result<()> {
    browse(
        source,
        runtime,
        query,
        projects,
        selected_file,
        Screen::new()?,
    )
}

fn browse(
    source: &dyn Source,
    runtime: &Runtime,
    mut query: FileQuery,
    mut projects: Vec<(String, ProjectRef, FilePage)>,
    selected_file: Option<ProjectFile>,
    mut screen: Screen,
) -> Result<()> {
    let mut current = 0;
    let mut selected: usize = 0;
    let mut status = String::new();
    let mut held = None;
    loop {
        let (label, reference, page) = &projects[current];
        let file_count = super::other_files(page, selected_file.as_ref()).count();
        screen.render(
            label,
            reference,
            page,
            selected_file.as_ref(),
            &query.loaders,
            selected,
            current,
            projects.len(),
            &status,
        )?;
        if held.is_some() && !event::poll(Duration::from_millis(180))? {
            held = None;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if held == Some(key.code) {
            if key.kind == KeyEventKind::Release {
                held = None;
            }
            continue;
        }
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        held = None;
        let offset = match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "file browsing interrupted",
                )
                .into());
            }
            KeyCode::Tab | KeyCode::BackTab => {
                current = if key.code == KeyCode::BackTab {
                    (current + projects.len() - 1) % projects.len()
                } else {
                    (current + 1) % projects.len()
                };
                selected = 0;
                status.clear();
                continue;
            }
            KeyCode::Up | KeyCode::Char('k') if selected > 0 => {
                selected -= 1;
                None
            }
            KeyCode::Down | KeyCode::Char('j') if selected + 1 < file_count => {
                selected += 1;
                None
            }
            KeyCode::Home => {
                selected = 0;
                None
            }
            KeyCode::End => {
                selected = file_count.saturating_sub(1);
                None
            }
            KeyCode::Right
            | KeyCode::PageDown
            | KeyCode::Char('n')
            | KeyCode::Down
            | KeyCode::Char('j') => {
                let next = page.offset.saturating_add(u64::from(query.limit));
                (!page.files.is_empty() && page.total.is_none_or(|total| next < total))
                    .then_some(next)
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
        if let Some(offset) = offset {
            held = Some(key.code);
            screen.render(
                label,
                reference,
                page,
                selected_file.as_ref(),
                &query.loaders,
                selected,
                current,
                projects.len(),
                "Loading...",
            )?;
            let backwards = offset < page.offset;
            query.offset = offset;
            match request::wait(
                runtime,
                source.list_files(reference, &query),
                "Fetching files...",
                |loading| {
                    screen.render(
                        label,
                        reference,
                        page,
                        selected_file.as_ref(),
                        &query.loaders,
                        selected,
                        current,
                        projects.len(),
                        loading,
                    )
                },
            ) {
                Ok(Some(next)) if !next.files.is_empty() => {
                    selected = if backwards {
                        super::other_files(&next, selected_file.as_ref())
                            .count()
                            .saturating_sub(1)
                    } else {
                        0
                    };
                    projects[current].2 = next;
                    status.clear();
                }
                Ok(Some(_)) => status = "No more files.".into(),
                Ok(None) => status = "Cancelled.".into(),
                Err(error) if request::is_interrupted(&error) => return Err(error),
                Err(error) => status = format!("{error:#}"),
            }
        }
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl Screen {
    fn new() -> io::Result<Self> {
        let mut screen = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stderr()))?,
        };
        terminal::enable_raw_mode()?;
        execute!(screen.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
        screen.terminal.clear()?;
        Ok(screen)
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        label: &str,
        project: &ProjectRef,
        page: &FilePage,
        selected_file: Option<&ProjectFile>,
        loaders: &[String],
        selected: usize,
        current: usize,
        count: usize,
        status: &str,
    ) -> io::Result<()> {
        self.terminal.draw(|frame| {
            let area = frame.area();
            let width = area.width.saturating_sub(4);
            let line = |row, value: String, role| {
                (
                    Paragraph::new(value).style(widgets::style(role)),
                    Rect::new(2, row, width, 1),
                )
            };
            if area.width < 28 || area.height < 10 {
                let (widget, rect) = line(0, "Enlarge terminal · q: quit".into(), Role::Muted);
                frame.render_widget(widget, rect);
                return;
            }
            let total = page
                .total
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".into());
            for (row, value, role) in [
                (
                    0,
                    format!(
                        "FILES · {} · {} / {}",
                        text(label),
                        text(&project.source.0),
                        text(&project.id.0)
                    ),
                    Role::Primary,
                ),
                (
                    1,
                    format!(
                        "Project {}/{} · {} files · offset {} · {total} matching",
                        current + 1,
                        count,
                        page.files.len(),
                        page.offset
                    ),
                    Role::Muted,
                ),
                (
                    2,
                    if loaders.is_empty() {
                        String::new()
                    } else {
                        format!("Loaders: {}", text(&loaders.join(", ")))
                    },
                    Role::Muted,
                ),
                (area.height - 2, text(status), Role::Muted),
                (
                    area.height - 1,
                    "Esc/q back · ↑↓ select/page · ←→ page · Tab project · Home/End".into(),
                    Role::Muted,
                ),
            ] {
                let (widget, rect) = line(row, value, role);
                frame.render_widget(widget, rect);
            }
            let bottom = area.height - 2;
            let mut top = 3;
            if let Some(file) = selected_file {
                let card = file_card(file, "Selected".into(), true);
                let height = card
                    .line_count(width.saturating_sub(2))
                    .min(usize::from(u16::MAX)) as u16;
                if top + height + 1 > bottom {
                    let (widget, rect) = line(
                        top,
                        "Enlarge terminal to show Selected and other files.".into(),
                        Role::Muted,
                    );
                    frame.render_widget(widget, rect);
                    return;
                }
                frame.render_widget(card, Rect::new(2, top, width, height));
                top += height + 1;
            }
            let cards: Vec<_> = super::other_files(page, selected_file)
                .enumerate()
                .map(|(position, (index, file))| {
                    let number = page.offset + index as u64 + 1;
                    let focused = position == selected;
                    let title = if focused {
                        format!("● {number}")
                    } else {
                        number.to_string()
                    };
                    let card = file_card(file, title, focused);
                    let height = card
                        .line_count(width.saturating_sub(2))
                        .min(usize::from(u16::MAX)) as u16;
                    (card, height)
                })
                .collect();
            let visible = bottom.saturating_sub(top);
            let mut first = 0;
            let mut used: u32 = cards
                .iter()
                .take(selected + 1)
                .map(|(_, height)| u32::from(*height))
                .sum();
            while first < selected && used > u32::from(visible) {
                used -= u32::from(cards[first].1);
                first += 1;
            }
            if cards.is_empty() {
                let message = if selected_file.is_some() {
                    "No other matching files."
                } else {
                    "No matching files."
                };
                let (widget, rect) = line(top, message.into(), Role::Muted);
                frame.render_widget(widget, rect);
            } else if cards
                .get(selected)
                .is_some_and(|(_, height)| *height > visible)
            {
                let (widget, rect) = line(
                    top,
                    "Enlarge terminal height to show the full file card.".into(),
                    Role::Muted,
                );
                frame.render_widget(widget, rect);
            } else {
                for (card, height) in cards.into_iter().skip(first) {
                    if top + height > bottom {
                        break;
                    }
                    frame.render_widget(card, Rect::new(2, top, width, height));
                    top += height;
                }
            }
        })?;
        Ok(())
    }
}

/// Build cards with library widgets so wrapping and borders share one layout.
fn file_card(file: &ProjectFile, title: String, focused: bool) -> Paragraph<'static> {
    let channel = match file.release_type {
        ReleaseType::Release => "release",
        ReleaseType::Beta => "beta",
        ReleaseType::Alpha => "alpha",
    };
    let mut lines = vec![
        Line::styled(text(&file.name), widgets::style(Role::Primary)),
        Line::styled(
            text(&format!(
                "ID {} · {} · {channel}",
                file.reference.id.0,
                file.version.as_deref().unwrap_or("unknown version")
            )),
            widgets::style(Role::Muted),
        ),
        Line::styled(
            format!("Minecraft {}", text(&file.game_versions.join(", "))),
            widgets::style(Role::Secondary),
        ),
        Line::styled(
            text(&format!(
                "{} · {} bytes · {} dependencies",
                file.filename,
                file.size,
                file.dependencies.len()
            )),
            widgets::style(Role::Muted),
        ),
    ];
    if !file.loaders.is_empty() {
        let mut loaders = Vec::new();
        for (index, loader) in file.loaders.iter().enumerate() {
            if index > 0 {
                loaders.push(Span::raw(" · "));
            }
            loaders.push(Span::styled(text(loader), widgets::loader(loader)));
        }
        lines.push(Line::from(loaders));
    }
    lines.push(Line::styled(
        text(
            file.download_url
                .as_ref()
                .map(|url| url.as_str())
                .unwrap_or("No direct download URL"),
        ),
        widgets::style(Role::Muted),
    ));
    Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(widgets::style(if focused {
                Role::Primary
            } else {
                Role::Muted
            }))
            .title(title),
    )
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
