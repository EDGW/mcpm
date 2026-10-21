//! Compact cards, bounded table cells and selectable mod details using ratatui.

use super::super::{
    columns::Column,
    rows::{Paint, Piece, Row},
};
use super::{Browser, DETAIL_FIELDS, DetailField, Focus, Summary, filter, identify};
use crate::theme::{
    Role,
    widgets::{self, style},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Cell, ListItem, Padding, Paragraph, Row as TableRow, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::widgets::{
    MatchedText,
    browser::{self, BrowserLayout, TableColumns, panel, short_line},
};

pub(super) fn draw(
    frame: &mut Frame,
    browser: &mut Browser,
    columns: &[Column],
    summary: &Summary<'_>,
) -> usize {
    let layout = BrowserLayout::new(
        frame.area(),
        browser.details || browser.ignored,
        browser.focus == Focus::Details,
        if browser.filter_open { 3 } else { 0 },
        0,
        10,
    );
    let (title, footer) = (layout.title, layout.footer);
    let title_text = format!(
        "Mod List · {} / {} rows{}",
        browser.matches.len(),
        browser.nodes.len(),
        if browser.query.trim().is_empty() {
            ""
        } else {
            " · Filtered"
        }
    );
    frame.render_widget(
        Paragraph::new(short_line(
            vec![Span::styled(title_text, style(Role::Primary))],
            title.width as usize,
        )),
        title,
    );
    browser.table_area = Rect::default();
    browser.detail_area = Rect::default();
    browser.filter_area = Rect::default();
    let page;
    if browser.details || browser.ignored {
        draw_details(frame, layout.details, browser, summary);
        page = layout.details.height.saturating_sub(2).max(1) as usize;
    } else {
        if layout.accessory.height > 0 {
            draw_filter(frame, layout.accessory, browser);
        }
        page = draw_table(frame, layout.table, browser, columns);
        if layout.details.height > 0 {
            draw_details(frame, layout.details, browser, summary);
        }
    }
    let status = browser
        .status
        .as_ref()
        .map(|(message, role)| (message.clone(), *role))
        .unwrap_or_else(|| {
            (
                format!(
                    "{} files skipped · {} input errors · {} file errors",
                    summary.ignored, summary.input_errors, summary.failed
                ),
                if summary.input_errors + summary.failed > 0 {
                    Role::Warning
                } else {
                    Role::Muted
                },
            )
        });
    let help = if browser.focus == Focus::Filter {
        "@field · = Exact · ~ Regex · & AND · | OR · () Group · Enter Apply · Esc / Q Close"
    } else if browser.focus == Focus::Details {
        if browser.selected_source_file().is_some() {
            "↑/↓ Field · c Copy · Enter View Release Files · Tab Mods"
        } else {
            "↑/↓ Field · c Copy · Tab Mods · Enter Full Details"
        }
    } else {
        "↑/↓ Move · ←/→ Collapse/Expand · Space Toggle · e Expand All"
    };
    let lines = [
        (status.0, status.1),
        (help.to_owned(), Role::Normal),
        (
            "Tab Switch Card · F Filter · r Reset · i Skipped · q / Esc Quit".to_owned(),
            Role::Muted,
        ),
    ]
    .into_iter()
    .map(|(text, role)| short_line(vec![Span::styled(text, style(role))], footer.width as usize))
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), footer);
    page
}

fn draw_filter(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    browser.filter_area = area;
    let focused = browser.focus == Focus::Filter;
    let parsed = &browser.parsed;
    let block = panel(
        parsed.error().map_or_else(
            || " Filter ".to_owned(),
            |error| format!(" Filter · {error} "),
        ),
        focused,
    )
    .border_style(style(if parsed.invalid() {
        Role::Warning
    } else if focused {
        Role::Primary
    } else {
        Role::Muted
    }))
    .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    let line = if browser.query.is_empty() && !focused {
        Line::from(Span::styled(
            "All names, IDs, files and loaders",
            style(Role::Muted),
        ))
    } else {
        filter_line(parsed)
    };
    let prefix =
        UnicodeWidthStr::width(&browser.query[..browser.query_cursor.min(browser.query.len())]);
    let scroll = if focused {
        prefix.saturating_sub(inner.width.saturating_sub(1) as usize)
    } else {
        0
    };
    let paragraph = Paragraph::new(line).scroll((0, scroll.min(u16::MAX as usize) as u16));
    frame.render_widget(paragraph.block(block), area);
    if focused && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + (prefix - scroll) as u16, inner.y));
    }
}

/// Token and diagnostic ranges originate in the parser; this adapter only assigns theme roles.
fn filter_line(parsed: &filter::Filter) -> Line<'_> {
    use ratatui::style::Modifier;
    let text = parsed.as_str();
    let mut boundaries = vec![0, text.len()];
    for span in &parsed.syntax {
        boundaries.extend([span.range.start, span.range.end]);
    }
    if let Some(error) = parsed.error() {
        boundaries.extend([error.range.start, error.range.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .map(|range| {
            let start = range[0];
            let end = range[1];
            let role = parsed
                .syntax
                .iter()
                .find(|span| span.range.contains(&start))
                .map_or(Role::Normal, |span| match span.kind {
                    filter::SyntaxKind::Field | filter::SyntaxKind::Escape => Role::Primary,
                    filter::SyntaxKind::Value => Role::Secondary,
                    filter::SyntaxKind::Operator => Role::Important,
                    filter::SyntaxKind::Delimiter => Role::Muted,
                });
            let paint = if parsed
                .error()
                .is_some_and(|error| error.range.contains(&start))
            {
                style(Role::Danger).add_modifier(Modifier::UNDERLINED)
            } else {
                style(role)
            };
            Span::styled(&text[start..end], paint)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn draw_table(frame: &mut Frame, area: Rect, browser: &mut Browser, columns: &[Column]) -> usize {
    browser.table_area = area;
    let block = panel(" Mods ", browser.focus == Focus::Mods);
    let inner = block.inner(area);
    let show_file = columns.contains(&Column::Filename);
    let visible_columns: Vec<_> = columns
        .iter()
        .copied()
        .filter(|column| *column != Column::Filename)
        .collect();
    let columns = visible_columns.as_slice();
    let weights = columns
        .iter()
        .map(|column| match column {
            Column::Index => 12,
            Column::Name => 28,
            Column::Modid => 20,
            Column::Filename => 28,
            Column::Loaders => 16,
        })
        .collect::<Vec<_>>();
    let sizing = TableColumns::new(inner.width, &weights, 2);
    let allocations = &sizing.areas;
    let (marker_width, spacing) = (sizing.marker_width, sizing.spacing);
    let header = sizing.header(
        &columns
            .iter()
            .map(|&column| label(column))
            .collect::<Vec<_>>(),
    );
    let expanded: Vec<_> = (0..browser.nodes.len())
        .map(|index| browser.is_expanded(index))
        .collect();
    let query = &browser.parsed;
    let spinner = browser.spinner();
    let rows = browser
        .matches
        .iter()
        .map(|&index| {
            let node = &browser.nodes[index];
            let marker = if node.children.is_empty() {
                ""
            } else if expanded[index] {
                "v"
            } else {
                ">"
            };
            let mut cells = vec![Cell::from(Span::styled(marker, style(Role::Muted)))];
            for ((position, &column), area) in columns.iter().enumerate().zip(allocations.iter()) {
                let indent = if position == 0 {
                    (node.depth * 2).min(area.width.saturating_sub(4) as usize)
                } else {
                    0
                };
                let mut spans = vec![Span::raw(" ".repeat(indent))];
                spans.extend(
                    field_text(query, column, &node.row)
                        .line(area.width as usize - indent)
                        .spans,
                );
                let first = Line::from(spans);
                if column == Column::Index && node.outer {
                    let badges = identify::Platform::ALL
                        .into_iter()
                        .enumerate()
                        .flat_map(|(position, platform)| {
                            let state = &browser.identification[node.file][platform.index()];
                            let text = match state {
                                identify::State::Matched(_) => "✓".to_owned(),
                                identify::State::Loading => spinner.clone(),
                                _ => " ".to_owned(),
                            };
                            [
                                Span::raw(if position == 0 { "" } else { " " }),
                                Span::styled(text, widgets::source(platform.id())),
                            ]
                        })
                        .collect();
                    cells.push(Cell::from(Text::from(vec![
                        first,
                        short_line(badges, area.width as usize),
                    ])));
                } else {
                    cells.push(Cell::from(first));
                }
            }
            TableRow::new(cells).height(2)
        })
        .collect::<Vec<_>>();
    let table = sizing.table(rows, header, block, browser.focus == Focus::Mods);
    frame.render_stateful_widget(table, area, &mut browser.table);
    if show_file {
        let file_column = columns
            .iter()
            .position(|column| *column == Column::Modid)
            .or_else(|| columns.iter().position(|column| *column != Column::Index))
            .unwrap_or(0);
        let mut file_x = inner
            .x
            .saturating_add(marker_width)
            .saturating_add(spacing)
            .saturating_add(allocations.get(file_column).map_or(0, |area| area.x))
            .saturating_add(file_column as u16 * spacing)
            .min(inner.right());
        // With only Index and File selected, keep the provider slots visible.
        if columns.get(file_column) == Some(&Column::Index) {
            file_x = file_x.saturating_add(4).min(inner.right());
        }
        let file_width = inner.right().saturating_sub(file_x);
        for (offset, &index) in browser
            .matches
            .iter()
            .skip(browser.table.offset())
            .enumerate()
        {
            let y = inner
                .y
                .saturating_add(2)
                .saturating_add((offset * 2).min(u16::MAX as usize) as u16)
                .saturating_add(1);
            if y >= inner.bottom() {
                break;
            }
            let text = field_text(query, Column::Filename, &browser.nodes[index].row);
            let selected = browser.focus == Focus::Mods
                && browser.table.selected() == Some(browser.table.offset() + offset);
            frame.render_widget(
                Paragraph::new(text.line(file_width as usize)).style(if selected {
                    widgets::selection()
                } else {
                    Style::default()
                }),
                Rect::new(file_x, y, file_width, 1),
            );
        }
    }
    if browser.matches.is_empty() {
        frame.render_widget(
            Paragraph::new(if browser.nodes.is_empty() {
                "No mods found."
            } else {
                "No matching mods. Press F to filter or r to reset."
            })
            .style(style(Role::Muted))
            .wrap(Wrap { trim: false }),
            Rect {
                y: inner.y.saturating_add(2),
                height: inner.height.saturating_sub(2),
                ..inner
            },
        );
    }
    (inner.height.saturating_sub(2) / 2).max(1) as usize
}

fn draw_details(frame: &mut Frame, area: Rect, browser: &mut Browser, summary: &Summary<'_>) {
    browser.detail_area = area;
    let block = panel(
        if browser.ignored {
            " Skipped files "
        } else {
            " Details "
        },
        browser.focus == Focus::Details,
    )
    .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    if browser.ignored {
        let lines = summary
            .ignored_files
            .iter()
            .map(|path| {
                Line::styled(
                    crate::commands::output_style::text(&path.display().to_string()),
                    style(Role::Muted),
                )
            })
            .collect::<Vec<_>>();
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let limit = paragraph
            .line_count(inner.width)
            .saturating_sub(inner.height as usize)
            .min(u16::MAX as usize) as u16;
        let scroll = browser.scroll.min(limit);
        frame.render_widget(paragraph.block(block).scroll((scroll, 0)), area);
        browser.scroll = scroll;
        browser.scroll_limit = limit;
        return;
    }
    let Some(node) = browser.selected_index().map(|index| &browser.nodes[index]) else {
        frame.render_widget(
            Paragraph::new("Select a mod to inspect its details.")
                .style(style(Role::Muted))
                .block(block),
            area,
        );
        return;
    };
    let query = &browser.parsed;
    let items = DETAIL_FIELDS
        .iter()
        .enumerate()
        .map(|(index, &field)| {
            if let DetailField::Source(platform) = field {
                return source_item(
                    platform,
                    &browser.identification[node.file][platform.index()],
                    node.outer,
                    &browser.spinner(),
                    inner.width as usize,
                    browser.focus == Focus::Details && browser.field == index,
                );
            }
            let DetailField::Column(column) = field else {
                unreachable!()
            };
            let mut spans = vec![Span::styled(
                format!("{}: ", field.label()),
                style(Role::Muted),
            )];
            let label_width = spans.iter().map(Span::width).sum::<usize>();
            spans.extend(
                field_text(query, column, &node.row)
                    .line((inner.width as usize).saturating_sub(label_width))
                    .spans,
            );
            ListItem::new(short_line(spans, inner.width as usize))
        })
        .collect::<Vec<_>>();
    browser::draw_fields(
        frame,
        area,
        block,
        items,
        browser.field,
        &mut browser.detail_offset,
        browser.focus == Focus::Details,
    );
    browser.scroll_limit = 0;
}

fn source_item(
    platform: identify::Platform,
    state: &identify::State,
    outer: bool,
    spinner: &str,
    width: usize,
    focused: bool,
) -> ListItem<'static> {
    let (status, explanation, role, actionable) = if !outer {
        (
            "Not checked".into(),
            "Nested JAR identification is not supported".into(),
            Role::Muted,
            false,
        )
    } else {
        match state {
            identify::State::Matched(_) => (
                "✓ Identified".into(),
                "↳ Enter to view release files →".into(),
                Role::Normal,
                true,
            ),
            identify::State::Queued => (
                "Waiting".into(),
                "Identification queued".into(),
                Role::Muted,
                false,
            ),
            identify::State::Loading => (
                format!("{spinner} Identifying…"),
                "Looking up this file".into(),
                Role::Muted,
                false,
            ),
            identify::State::Unmatched => (
                "No match".into(),
                "No matching release file found".into(),
                Role::Muted,
                false,
            ),
            identify::State::Failed(error) => (
                "Failed".into(),
                crate::commands::output_style::text(error),
                Role::Danger,
                false,
            ),
        }
    };
    let status_style = if actionable {
        widgets::source(platform.id())
    } else {
        style(role)
    };
    let mut heading = vec![
        Span::styled(
            format!("{}: ", platform.name()),
            widgets::source(platform.id()),
        ),
        Span::styled(status, status_style),
    ];
    if outer && let identify::State::Matched(found) = state {
        heading.push(Span::styled(
            format!(
                " · Project ID: {} · File ID: {}",
                crate::commands::output_style::text(&found.reference.project.id.0),
                crate::commands::output_style::text(&found.reference.id.0)
            ),
            style(Role::Muted),
        ));
    }
    ListItem::new(Text::from(vec![
        short_line(heading, width),
        short_line(
            vec![Span::styled(
                format!("  {explanation}"),
                style(if actionable && focused {
                    Role::Primary
                } else {
                    Role::Muted
                }),
            )],
            width,
        ),
    ]))
}

fn piece(value: &Piece) -> Span<'_> {
    Span::styled(
        value.text.as_str(),
        match &value.style {
            Paint::Plain => Style::default(),
            Paint::Role(role) => widgets::style(*role),
            Paint::Loader(id) => widgets::loader(id),
        },
    )
}

/// Adapt semantic field content to the generic widget; search syntax and theme
/// selection stay in the command frontend rather than leaking into the widget.
fn field_text<'a>(query: &filter::Filter, column: Column, row: &'a Row) -> MatchedText<'a> {
    let pieces = &row.cells[column.position()];
    let spans: Vec<_> = pieces.iter().map(piece).collect();
    MatchedText::new(Line::from(spans))
        .matches(query.ranges(row, column))
        .match_style(style(Role::Secondary))
        .ellipsis_style(style(Role::Muted))
}

pub(super) fn label(column: Column) -> &'static str {
    match column {
        Column::Index => "Index",
        Column::Name => "Name",
        Column::Modid => "Mod ID",
        Column::Filename => "File",
        Column::Loaders => "Loaders",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn styled_suffix_and_wide_text_are_clipped_as_one_cell() {
        for width in 0..30 {
            let line = short_line(
                vec![
                    Span::raw("[Forge]"),
                    Span::raw(" (+999 mods)"),
                    Span::raw(" 中文"),
                ],
                width,
            );
            assert!(line.width() <= width);
            if (3..23).contains(&width) {
                assert!(line.to_string().ends_with("..."));
            }
        }
        assert_eq!(
            short_line(vec![Span::raw("中文"), Span::raw("suffix")], 6).to_string(),
            "中..."
        );
    }
}
