//! Compact cards, bounded table cells and selectable mod details using ratatui.

use super::super::{
    columns::Column,
    rows::{Paint, Piece},
};
use super::{Browser, DETAIL_FIELDS, DetailField, Focus, Summary, filter, identify};
use crate::theme::{
    Role,
    widgets::{self, style},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{
        Block, Cell, List, ListItem, ListState, Padding, Paragraph, Row as TableRow, Table, Wrap,
    },
};
use unicode_width::UnicodeWidthStr;

use crate::widgets::{MatchedText, Overflow};

pub(super) fn draw(
    frame: &mut Frame,
    browser: &mut Browser,
    columns: &[Column],
    summary: &Summary<'_>,
) -> usize {
    let [title, content, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(frame.area());
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
        draw_details(frame, content, browser, summary);
        page = content.height.saturating_sub(2).max(1) as usize;
    } else {
        // Details remain below the list, including in wide terminals.
        let detail_height = if content.height >= 14 {
            8
        } else if browser.focus == Focus::Details {
            content.height.min(8)
        } else {
            0
        };
        let [table, filter_area, detail] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(if browser.filter_open { 3 } else { 0 }),
            Constraint::Length(detail_height),
        ])
        .areas(content);
        if filter_area.height > 0 {
            draw_filter(frame, filter_area, browser);
        }
        page = draw_table(frame, table, browser, columns);
        if detail.height > 0 {
            draw_details(frame, detail, browser, summary);
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
        "@name @modid @file @loaders · Enter Apply · Esc / Q Close"
    } else if browser.focus == Focus::Details {
        if browser.selected_source_file().is_some() {
            "↑/↓ Field · c Copy · Enter Files · Tab Mods"
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
    let parsed = filter::Query::parse(&browser.query);
    let block = Block::bordered()
        .title(if parsed.invalid() {
            " Filter · Unknown field "
        } else {
            " Filter "
        })
        .border_style(style(if parsed.invalid() {
            Role::Warning
        } else if focused {
            Role::Primary
        } else {
            Role::Muted
        }))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    let mut spans = Vec::new();
    if browser.query.is_empty() && !focused {
        spans.push(Span::styled(
            "All names, IDs, files and loaders",
            style(Role::Muted),
        ));
    }
    for part in &parsed.parts {
        if let Some(marker) = &part.marker {
            spans.push(Span::styled(
                &browser.query[marker.clone()],
                style(if part.field.is_some() {
                    Role::Primary
                } else {
                    Role::Warning
                }),
            ));
        }
        spans.push(Span::styled(
            &browser.query[part.content.clone()],
            style(if part.marker.is_some() {
                Role::Secondary
            } else {
                Role::Normal
            }),
        ));
    }
    let prefix =
        UnicodeWidthStr::width(&browser.query[..browser.query_cursor.min(browser.query.len())]);
    let scroll = if focused {
        prefix.saturating_sub(inner.width.saturating_sub(1) as usize)
    } else {
        0
    };
    let paragraph =
        Paragraph::new(Line::from(spans)).scroll((0, scroll.min(u16::MAX as usize) as u16));
    frame.render_widget(paragraph.block(block), area);
    if focused && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + (prefix - scroll) as u16, inner.y));
    }
}

fn draw_table(frame: &mut Frame, area: Rect, browser: &mut Browser, columns: &[Column]) -> usize {
    browser.table_area = area;
    let block =
        Block::bordered()
            .title(" Mods ")
            .border_style(style(if browser.focus == Focus::Mods {
                Role::Primary
            } else {
                Role::Muted
            }));
    let inner = block.inner(area);
    let show_file = columns.contains(&Column::Filename);
    let visible_columns: Vec<_> = columns
        .iter()
        .copied()
        .filter(|column| *column != Column::Filename)
        .collect();
    let columns = visible_columns.as_slice();
    let marker_width = inner.width.min(2);
    let spacing = if inner.width > columns.len() as u16 * 3 {
        1
    } else {
        0
    };
    let available = inner
        .width
        .saturating_sub(marker_width)
        .saturating_sub(spacing * columns.len() as u16);
    let constraints = columns
        .iter()
        .map(|column| {
            Constraint::Fill(match column {
                Column::Index => 12,
                Column::Name => 28,
                Column::Modid => 20,
                Column::Filename => 28,
                Column::Loaders => 16,
            })
        })
        .collect::<Vec<_>>();
    // Allocate exact lengths once for both clipping and table rendering, so styled
    // suffixes such as (+X mods) cannot escape their cell's rectangle.
    let allocations = Layout::horizontal(constraints).split(Rect::new(0, 0, available, 1));
    let mut widths = vec![Constraint::Length(marker_width)];
    widths.extend(
        allocations
            .iter()
            .map(|area| Constraint::Length(area.width)),
    );
    let mut header_cells = vec![Cell::from("")];
    header_cells.extend(
        columns
            .iter()
            .zip(allocations.iter())
            .map(|(&column, area)| {
                Cell::from(short_line(
                    vec![Span::raw(label(column))],
                    area.width as usize,
                ))
            }),
    );
    let header = TableRow::new(header_cells)
        .style(style(Role::Muted))
        .bottom_margin(1);
    let expanded: Vec<_> = (0..browser.nodes.len())
        .map(|index| browser.is_expanded(index))
        .collect();
    let query = filter::Query::parse(&browser.query);
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
                    field_text(&query, column, &node.row.cells[column.position()])
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
    let table = Table::new(rows, widths)
        .block(block)
        .header(header)
        .column_spacing(spacing)
        .row_highlight_style(if browser.focus == Focus::Mods {
            widgets::selection()
        } else {
            Style::default()
        });
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
            let text = field_text(
                &query,
                Column::Filename,
                &browser.nodes[index].row.cells[Column::Filename.position()],
            );
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
    let block = Block::bordered()
        .title(if browser.ignored {
            " Skipped files "
        } else {
            " Details "
        })
        .border_style(style(if browser.focus == Focus::Details {
            Role::Primary
        } else {
            Role::Muted
        }))
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
    let query = filter::Query::parse(&browser.query);
    let items = DETAIL_FIELDS
        .iter()
        .map(|&field| {
            let mut spans = vec![Span::styled(
                format!("{}: ", field.label()),
                match field {
                    DetailField::Source(platform) => widgets::source(platform.id()),
                    _ => style(Role::Muted),
                },
            )];
            match field {
                DetailField::Column(column) => {
                    let label_width = spans.iter().map(Span::width).sum::<usize>();
                    spans.extend(
                        field_text(&query, column, &node.row.cells[column.position()])
                            .line((inner.width as usize).saturating_sub(label_width))
                            .spans,
                    );
                }
                DetailField::Source(platform) => {
                    let state = &browser.identification[node.file][platform.index()];
                    let color = if !node.outer {
                        style(Role::Muted)
                    } else {
                        match state {
                            identify::State::Matched(_) | identify::State::Loading => {
                                widgets::source(platform.id())
                            }
                            identify::State::Failed(_) => style(Role::Danger),
                            _ => style(Role::Muted),
                        }
                    };
                    let value =
                        crate::commands::output_style::text(&browser.detail_value(field).unwrap());
                    if node.outer
                        && matches!(
                            state,
                            identify::State::Matched(_) | identify::State::Loading
                        )
                    {
                        let end = value.chars().next().map_or(0, char::len_utf8);
                        spans.push(Span::styled(value[..end].to_owned(), color));
                        spans.push(Span::styled(value[end..].to_owned(), style(Role::Muted)));
                    } else {
                        spans.push(Span::styled(value, color));
                    }
                }
            }
            ListItem::new(short_line(spans, inner.width as usize))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default()
        .with_offset(browser.detail_offset)
        .with_selected(Some(browser.field));
    let list = List::new(items)
        .block(block)
        .highlight_style(if browser.focus == Focus::Details {
            widgets::selection()
        } else {
            Style::default()
        });
    frame.render_stateful_widget(list, area, &mut state);
    browser.detail_offset = state.offset();
    browser.scroll_limit = 0;
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
fn field_text<'a>(query: &filter::Query, column: Column, pieces: &'a [Piece]) -> MatchedText<'a> {
    let spans: Vec<_> = pieces.iter().map(piece).collect();
    let text: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
    MatchedText::new(Line::from(spans))
        .matches(query.ranges(column, &text))
        .match_style(style(Role::Secondary))
        .ellipsis_style(style(Role::Muted))
}

/// Non-search labels share the same Unicode-safe clipping implementation.
fn short_line(spans: Vec<Span<'_>>, width: usize) -> Line<'static> {
    MatchedText::new(Line::from(spans))
        .overflow(Overflow::Ellipsis)
        .ellipsis_style(style(Role::Muted))
        .line(width)
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
