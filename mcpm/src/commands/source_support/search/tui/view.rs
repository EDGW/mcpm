//! Search field, compact project list, and bottom details following the mod list layout.

use std::io;

use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use kako_mods_lib::source::{ProjectInfo, SearchPage, SearchQuery};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Cell, Padding, Paragraph, Row, Table, TableState, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{Browser, Focus};
use crate::{
    commands::output_style::text,
    theme::{
        Role,
        widgets::{self, style},
    },
    widgets::{MatchedText, Overflow},
};

pub(super) struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl Screen {
    pub(super) fn new() -> io::Result<Self> {
        let mut screen = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stderr()))?,
        };
        terminal::enable_raw_mode()?;
        execute!(screen.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
        screen.terminal.clear()?;
        Ok(screen)
    }

    pub(super) fn render(
        &mut self,
        source: &str,
        query: &SearchQuery,
        page: &SearchPage,
        browser: &mut Browser,
        status: &str,
    ) -> io::Result<()> {
        self.terminal
            .draw(|frame| draw(frame, source, query, page, browser, status))?;
        Ok(())
    }
}

pub(super) fn draw(
    frame: &mut Frame,
    source: &str,
    query: &SearchQuery,
    page: &SearchPage,
    browser: &mut Browser,
    status: &str,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal · Esc/q quit · Ctrl-C exit").style(style(Role::Muted)),
            area,
        );
        return;
    }
    let [search, metadata, results, details, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(8.min(area.height - 12)),
        Constraint::Length(3),
    ])
    .areas(area);
    draw_search(frame, search, browser, query);

    let range = if page.projects.is_empty() {
        "0".into()
    } else {
        format!(
            "{}–{}",
            page.offset + 1,
            page.offset + page.projects.len() as u64
        )
    };
    let total = page
        .total
        .map_or_else(|| "?".into(), |total| total.to_string());
    let mut filters = vec![Span::styled(text(source), widgets::source(source))];
    if let Some(version) = &query.game_version {
        filters.push(Span::styled(
            format!(" · Minecraft {}", text(version)),
            style(Role::Muted),
        ));
    }
    if let Some(loader) = &query.loader {
        filters.push(Span::raw(" · "));
        filters.push(Span::styled(text(loader), widgets::loader(loader)));
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(filters),
            Line::styled(
                format!(
                    "Results {range} of {total} · Search: {}",
                    text(&query.query)
                ),
                style(Role::Muted),
            ),
        ]),
        metadata,
    );

    draw_results(frame, results, page, browser);
    draw_details(frame, details, page.projects.get(browser.selected), browser);
    let help = match browser.focus {
        Focus::Search => "Enter search · Esc discard · Tab switch",
        Focus::Results => "Enter files · / search · Tab switch · Esc/q quit",
        Focus::Details => "↑↓ scroll · Home/End · Tab switch · Esc/q quit",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                if page.projects.is_empty() {
                    String::new()
                } else {
                    format!(
                        "Selected {} / {} · ↑↓ select / cross-page · ←→ page",
                        browser.selected + 1,
                        page.projects.len()
                    )
                },
                style(Role::Muted),
            ),
            Line::styled(text(status), style(Role::Warning)),
            Line::styled(help, style(Role::Muted)),
        ]),
        footer,
    );
}

fn draw_search(frame: &mut Frame, area: Rect, browser: &Browser, query: &SearchQuery) {
    let focused = browser.focus == Focus::Search;
    let title = if browser.draft != query.query {
        " Search · edited · Enter to search "
    } else {
        " Search · / to edit "
    };
    let block = panel(title, focused);
    let inner = block.inner(area);
    let value = text(&browser.draft);
    let prefix = text(&browser.draft[..browser.cursor]).width();
    // Start on a character boundary so scrolling never cuts a wide glyph in half.
    let mut start = 0;
    let mut skipped = 0;
    if focused {
        for (index, ch) in value.char_indices() {
            if prefix.saturating_sub(skipped) < usize::from(inner.width) {
                break;
            }
            skipped += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            start = index + ch.len_utf8();
        }
    }
    frame.render_widget(
        Paragraph::new(&value[start..])
            .style(style(Role::Normal))
            .block(block),
        area,
    );
    if focused && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + prefix.saturating_sub(skipped) as u16, inner.y));
    }
}

fn draw_results(frame: &mut Frame, area: Rect, page: &SearchPage, browser: &mut Browser) {
    let focused = browser.focus == Focus::Results;
    let block = panel(" Results ", focused);
    let inner = block.inner(area);
    let columns = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).split(Rect::new(
        0,
        0,
        inner.width.saturating_sub(1),
        1,
    ));
    let cell = |value: &str, role, width| {
        Cell::from(
            MatchedText::new(Line::styled(text(value), style(role)))
                .overflow(Overflow::Ellipsis)
                .ellipsis_style(style(Role::Muted))
                .line(usize::from(width)),
        )
    };
    let header = Row::new(vec![
        cell("Project Name", Role::Muted, columns[0].width),
        cell("slug", Role::Muted, columns[1].width),
    ]);
    let rows = page.projects.iter().map(|project| {
        Row::new(vec![
            cell(&project.name, Role::Primary, columns[0].width),
            cell(
                project.slug.as_deref().unwrap_or("—"),
                Role::Secondary,
                columns[1].width,
            ),
        ])
        .height(if inner.height >= 6 { 2 } else { 1 })
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(columns[0].width),
            Constraint::Length(columns[1].width),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(block)
    .row_highlight_style(if focused {
        widgets::selection()
    } else {
        Style::default()
    });
    let mut state = TableState::default()
        .with_offset(browser.result_offset)
        .with_selected((!page.projects.is_empty()).then_some(browser.selected));
    frame.render_stateful_widget(table, area, &mut state);
    browser.result_offset = state.offset();
    if page.projects.is_empty() {
        frame.render_widget(
            Paragraph::new("No mods found. Press / to search.")
                .style(style(Role::Muted))
                .wrap(Wrap { trim: false }),
            Rect {
                y: inner.y + 1,
                height: inner.height.saturating_sub(1),
                ..inner
            },
        );
    }
}

fn draw_details(
    frame: &mut Frame,
    area: Rect,
    project: Option<&ProjectInfo>,
    browser: &mut Browser,
) {
    let block = panel(" Details ", browser.focus == Focus::Details);
    let inner = block.inner(area);
    let lines = project.map_or_else(
        || {
            vec![Line::styled(
                "Select a project to view details.",
                style(Role::Muted),
            )]
        },
        |project| {
            vec![
                detail_field("Project Name", &project.name, Role::Primary),
                detail_field(
                    "slug",
                    project.slug.as_deref().unwrap_or("—"),
                    Role::Secondary,
                ),
                Line::from(vec![
                    Span::styled("Source: ", style(Role::Muted)),
                    Span::styled(
                        text(&project.reference.source.0),
                        widgets::source(&project.reference.source.0),
                    ),
                ]),
                detail_field("ID", &project.reference.id.0, Role::Muted),
                detail_field("Summary", &project.summary, Role::Normal),
                detail_field(
                    "Website",
                    project.website_url.as_ref().map_or("—", |url| url.as_str()),
                    Role::Muted,
                ),
            ]
        },
    );
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    browser.detail_limit = paragraph
        .line_count(inner.width)
        .saturating_sub(usize::from(inner.height))
        .min(usize::from(u16::MAX)) as u16;
    browser.detail_scroll = browser.detail_scroll.min(browser.detail_limit);
    frame.render_widget(
        paragraph.scroll((browser.detail_scroll, 0)).block(block),
        area,
    );
}

fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_style(style(if focused { Role::Primary } else { Role::Muted }))
        .padding(Padding::horizontal(1))
        .title(title)
}

fn detail_field(label: &str, value: &str, role: Role) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), style(Role::Muted)),
        Span::styled(text(value), style(role)),
    ])
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
