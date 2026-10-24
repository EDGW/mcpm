//! Search-specific content rendered with shared browser layout, tables, fields and terminal ownership.

use std::io;

use clap::ValueEnum;
use mcpm_backend::source::{ProjectInfo, SearchPage, SearchQuery};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span, Text},
    widgets::{Cell, ListItem, Padding, Paragraph, Row, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::super::expression::{Expression, ParseError, SyntaxKind, SyntaxSpan};
use super::super::versions;
use super::{Browser, Focus};
use crate::{
    commands::supports::mods::loaders::Loader,
    display::style::text,
    theme::{
        Role,
        widgets::{self, style},
    },
    widgets::{
        browser::{self, BrowserLayout, Screen, TableColumns, panel, short_line},
        highlighted_line,
    },
};

pub(super) const FIELD_COUNT: usize = 9;
pub(super) const TABLE_HEADER_HEIGHT: u16 = 3;

pub(super) fn field_value(project: &ProjectInfo, field: usize) -> (&'static str, String) {
    match field {
        0 => ("Project Name", project.name.clone()),
        1 => ("slug", project.slug.clone().unwrap_or_else(|| "—".into())),
        2 => ("Source", project.reference.source.0.clone()),
        3 => ("ID", project.reference.id.0.clone()),
        4 => ("Loaders", values(&visible_loaders(&project.loaders))),
        5 => (
            "Minecraft",
            values(&versions::expanded(&project.game_versions)),
        ),
        6 => ("Categories", values(&project.categories)),
        7 => ("Summary", project.summary.clone()),
        _ => (
            "Website",
            project
                .website_url
                .as_ref()
                .map_or_else(|| "—".into(), |url| url.to_string()),
        ),
    }
}

fn values(values: &[String]) -> String {
    if values.is_empty() {
        "—".into()
    } else {
        values.join(", ")
    }
}

fn version_spans(value: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, part) in value.split(',').enumerate() {
        if index > 0 {
            spans.push(Span::styled(",", style(Role::Normal)));
        }
        spans.push(Span::styled(part.to_owned(), style(Role::Muted)));
    }
    spans
}

fn result_versions(
    project: &ProjectInfo,
    catalog: Option<&[String]>,
    width: usize,
) -> Line<'static> {
    let count = versions::expanded(&project.game_versions).len();
    if count == 0 {
        return short_line(version_spans("—"), width);
    }
    let compact = versions::compact(&project.game_versions, catalog);
    let summary = compact.into_iter().take(3).collect::<Vec<_>>().join(", ");
    let total = format!(" (all {count} versions)");
    // Reserve room for the real version count even when a terminal clips the three items.
    let mut line = short_line(
        version_spans(&text(&summary)),
        width.saturating_sub(total.width()),
    );
    line.spans.push(Span::styled(total, style(Role::Muted)));
    short_line(line.spans, width)
}

fn visible_loaders(loaders: &[String]) -> Vec<String> {
    Loader::value_variants()
        .iter()
        .map(|loader| loader.id())
        .filter(|id| loaders.iter().any(|loader| loader.eq_ignore_ascii_case(id)))
        .map(str::to_owned)
        .collect()
}

fn loader_spans(loaders: &[String]) -> Vec<Span<'static>> {
    let loaders = visible_loaders(loaders);
    if loaders.is_empty() {
        return vec![Span::styled("—", style(Role::Muted))];
    }
    let mut spans = Vec::new();
    for (index, loader) in loaders.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(", ", style(Role::Muted)));
        }
        spans.push(Span::styled(text(loader), widgets::loader(loader)));
    }
    spans
}

pub(super) fn render(
    screen: &mut Screen,
    source: &str,
    query: &SearchQuery,
    page: &SearchPage,
    browser: &mut Browser,
    loading: &str,
) -> io::Result<()> {
    screen
        .terminal
        .draw(|frame| draw(frame, source, query, page, browser, loading))?;
    Ok(())
}

pub(super) fn draw(
    frame: &mut Frame,
    source: &str,
    query: &SearchQuery,
    page: &SearchPage,
    browser: &mut Browser,
    loading: &str,
) {
    browser.search_area = Rect::default();
    browser.table_area = Rect::default();
    browser.detail_area = Rect::default();
    browser.completion.area = Rect::default();
    let area = frame.area();
    if area.width < 28 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal · Esc/q quit · Ctrl-C exit").style(style(Role::Muted)),
            area,
        );
        return;
    }
    // Search is command-specific and stays above the shared browser's title and content.
    let [search, content] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    browser.search_area = search;
    draw_search(frame, search, browser);
    let layout = BrowserLayout::new(
        content,
        browser.full_details,
        browser.focus == Focus::Details,
        0,
        0,
        8,
    );
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
    let mut title = vec![
        Span::styled(
            format!("Mod Search · {range} / {total} · "),
            style(Role::Primary),
        ),
        Span::styled(text(source), widgets::source(source)),
    ];
    if !query.game_versions.is_empty() {
        title.push(Span::styled(
            format!(" · Minecraft {}", text(&query.game_versions.join(" | "))),
            style(Role::Muted),
        ));
    }
    for (index, loader) in query.loaders.iter().enumerate() {
        title.push(Span::styled(
            if index == 0 { " · " } else { " | " },
            style(Role::Muted),
        ));
        title.push(Span::styled(text(loader), widgets::loader(loader)));
    }
    frame.render_widget(
        Paragraph::new(short_line(title, layout.title.width as usize)),
        layout.title,
    );
    if !browser.full_details {
        draw_results(frame, layout.table, page, browser);
    }
    if layout.details.height > 0 {
        draw_details(frame, layout.details, page, browser);
    }
    let context = format!(
        "Selected {} / {} · Search: {}",
        browser.table.selected().map_or(0, |index| index + 1),
        page.projects.len(),
        text(&query.query)
    );
    let (status, role) = if !loading.is_empty() {
        (loading, Role::Muted)
    } else {
        browser
            .status
            .as_ref()
            .map_or((context.as_str(), Role::Muted), |(message, role)| {
                (message.as_str(), *role)
            })
    };
    let help = match browser.focus {
        Focus::Search if browser.completion.open() => {
            "↑↓ Suggestion · Enter/Tab Insert · Esc Close · Ctrl-R Retry Versions"
        }
        Focus::Search => "Enter Search · Esc Discard · Tab Switch Card",
        Focus::Results => "↑↓ Select / Cross-page · ←→ / n/p Page · Enter Files",
        Focus::Details => "↑↓ Field · c Copy · Enter Full Details · Home/End",
    };
    frame.render_widget(
        Paragraph::new(vec![
            short_line(
                vec![Span::styled(text(status), style(role))],
                layout.footer.width as usize,
            ),
            short_line(
                vec![Span::styled(help, style(Role::Normal))],
                layout.footer.width as usize,
            ),
            short_line(
                vec![Span::styled(
                    "/ Search · Tab Switch Card · q / Esc Quit",
                    style(Role::Muted),
                )],
                layout.footer.width as usize,
            ),
        ]),
        layout.footer,
    );
    browser.completion.draw(frame, browser.search_area);
}

fn draw_search(frame: &mut Frame, area: Rect, browser: &Browser) {
    let focused = browser.focus == Focus::Search;
    let (parsed, syntax) = Expression::inspect(&browser.draft);
    let error = parsed.err();
    let title = if let Some(error) = &error {
        format!(" Search · {} at column {} ", error.message, error.column)
    } else if browser.draft != browser.committed {
        " Search · edited · Enter to search ".into()
    } else {
        " Search · / to edit ".into()
    };
    let block = panel(title, focused)
        .padding(Padding::horizontal(1))
        .border_style(style(if error.is_some() {
            Role::Warning
        } else if focused {
            Role::Primary
        } else {
            Role::Muted
        }));
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
        Paragraph::new(search_line(&browser.draft, &syntax, error.as_ref(), start))
            .style(style(Role::Normal))
            .block(block),
        area,
    );
    if focused && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + prefix.saturating_sub(skipped) as u16, inner.y));
    }
}

/// Keep parser ranges in the original draft; escape controls and clip only after styling.
fn search_line(
    source: &str,
    syntax: &[SyntaxSpan],
    error: Option<&ParseError>,
    start: usize,
) -> Line<'static> {
    let spans = syntax.iter().map(|span| {
        let role = match span.kind {
            SyntaxKind::Field | SyntaxKind::Escape => Role::Primary,
            SyntaxKind::Value => Role::Secondary,
            SyntaxKind::Operator => Role::Important,
            SyntaxKind::Delimiter => Role::Muted,
        };
        (span.range.clone(), style(role))
    });
    let danger = style(Role::Danger).add_modifier(Modifier::UNDERLINED);
    let diagnostic = error.map(|error| (error.range.clone(), danger));
    let mut line = highlighted_line(source, spans.chain(diagnostic));
    if error.is_some_and(|error| error.range.is_empty()) {
        line.spans.push(Span::styled(" ", danger));
    }
    let mut remaining = start;
    Line::from(
        line.spans
            .into_iter()
            .filter_map(|span| {
                let value = text(&span.content);
                let skip = remaining.min(value.len());
                remaining -= skip;
                (skip < value.len()).then(|| Span::styled(value[skip..].to_owned(), span.style))
            })
            .collect::<Vec<_>>(),
    )
}

fn draw_results(frame: &mut Frame, area: Rect, page: &SearchPage, browser: &mut Browser) {
    browser.table_area = area;
    let focused = browser.focus == Focus::Results;
    let block = panel(" Results ", focused);
    let inner = block.inner(area);
    let sizing = TableColumns::new(inner.width, &[1], 0);
    let top = TableColumns::new(inner.width, &[3, 2], 0);
    let header = Row::new([Cell::from(Text::from(vec![
        top.line([vec![Span::raw("Project Name")], vec![Span::raw("Loaders")]]),
        Line::from("Minecraft versions"),
    ]))])
    .height(2)
    .bottom_margin(1)
    .style(style(Role::Muted));
    browser.row_height = 2;
    let rows = page.projects.iter().map(|project| {
        Row::new([Cell::from(Text::from(vec![
            top.line([
                vec![Span::styled(text(&project.name), style(Role::Primary))],
                loader_spans(&project.loaders),
            ]),
            result_versions(
                project,
                browser.completion.version_catalog(),
                inner.width as usize,
            ),
        ]))])
        .height(browser.row_height)
    });
    // The shared Table owns scrolling; the command only provides rows and selection.
    browser.table.select(
        (!page.projects.is_empty()).then_some(
            browser
                .table
                .selected()
                .unwrap_or(0)
                .min(page.projects.len().saturating_sub(1)),
        ),
    );
    frame.render_stateful_widget(
        sizing.table(rows, header, block, focused),
        area,
        &mut browser.table,
    );
    if page.projects.is_empty() {
        frame.render_widget(
            Paragraph::new("No mods found. Press / to search.")
                .style(style(Role::Muted))
                .wrap(Wrap { trim: false }),
            Rect {
                y: inner.y.saturating_add(TABLE_HEADER_HEIGHT),
                height: inner.height.saturating_sub(TABLE_HEADER_HEIGHT),
                ..inner
            },
        );
    }
}

fn draw_details(frame: &mut Frame, area: Rect, page: &SearchPage, browser: &mut Browser) {
    browser.detail_area = area;
    let block = panel(" Details ", browser.focus == Focus::Details);
    let inner = block.inner(area);
    let Some(project) = browser.project(page) else {
        frame.render_widget(
            Paragraph::new("Select a project to view details.")
                .style(style(Role::Muted))
                .block(block),
            area,
        );
        return;
    };
    let width = inner.width.saturating_sub(2).max(1) as usize;
    browser.detail_fields.clear();
    let mut items = Vec::new();
    for field in 0..FIELD_COUNT {
        let (label, value) = field_value(project, field);
        if field == 5 {
            let label = format!("{label}: ");
            let content = format!("{label}{}", text(&value));
            for (index, line) in textwrap::wrap(&content, width).into_iter().enumerate() {
                let spans = if index == 0 {
                    let mut spans = vec![Span::styled(label.clone(), style(Role::Muted))];
                    spans.extend(version_spans(line.strip_prefix(&label).unwrap_or(&line)));
                    spans
                } else {
                    version_spans(&line)
                };
                items.push(ListItem::new(Line::from(spans)));
                browser.detail_fields.push(field);
            }
            continue;
        }
        let paint = match field {
            0 => style(Role::Primary),
            1 => style(Role::Secondary),
            2 => widgets::source(&project.reference.source.0),
            6 | 7 => style(Role::Normal),
            _ => style(Role::Muted),
        };
        let mut spans = vec![Span::styled(format!("{label}: "), style(Role::Muted))];
        if field == 4 {
            spans.extend(loader_spans(&project.loaders));
        } else {
            spans.push(Span::styled(text(&value), paint));
        }
        items.push(ListItem::new(short_line(spans, width)));
        browser.detail_fields.push(field);
    }
    // A wrapped field occupies several shared List rows; copying still uses its logical field.
    if browser.detail_fields.get(browser.detail_row) != Some(&browser.field) {
        browser.detail_row = browser
            .detail_fields
            .iter()
            .position(|&field| field == browser.field)
            .unwrap_or(0);
    }
    browser::draw_fields(
        frame,
        area,
        block,
        items,
        browser.detail_row,
        &mut browser.detail_offset,
        browser.focus == Focus::Details,
    );
}
