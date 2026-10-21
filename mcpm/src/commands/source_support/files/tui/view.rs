//! Compact two-line file rows and selectable details, using the mod list components.

use super::{Browser, Focus};
use crate::{
    commands::output_style::text,
    theme::{
        Role,
        widgets::{self, style},
    },
    widgets::browser::{self, BrowserLayout, TableColumns, panel, short_line},
};
use kako_mods_lib::source::{FilePage, FileQuery, ProjectFile, ProjectRef, ReleaseType};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Span, Text},
    widgets::{Cell, ListItem, Padding, Paragraph, Row},
};

pub(super) const FIELD_COUNT: usize = 14;
// Two header lines followed by a blank separator.
pub(super) const TABLE_HEADER_HEIGHT: u16 = 3;

pub(super) fn field_value(file: &ProjectFile, field: usize) -> (&'static str, String) {
    match field {
        0 => ("Name", file.name.clone()),
        1 => (
            "Version",
            file.version
                .clone()
                .unwrap_or_else(|| "unknown version".into()),
        ),
        2 => ("File ID", file.reference.id.0.clone()),
        3 => ("Source", file.reference.project.source.0.clone()),
        4 => ("Project", file.reference.project.id.0.clone()),
        5 => ("Release", channel(file.release_type).into()),
        6 => ("Minecraft", file.game_versions.join(", ")),
        7 => ("Loaders", file.loaders.join(", ")),
        8 => ("Filename", file.filename.clone()),
        9 => ("Size", format!("{} bytes", file.size)),
        10 => ("Dependencies", file.dependencies.len().to_string()),
        11 => (
            "Dependency targets",
            file.dependencies
                .iter()
                .map(|dependency| {
                    use kako_mods_lib::source::DependencyTarget;
                    let target = match &dependency.target {
                        DependencyTarget::Project(project) => {
                            format!("{} / {}", project.source.0, project.id.0)
                        }
                        DependencyTarget::File(file) => format!(
                            "{} / {} / {}",
                            file.project.source.0, file.project.id.0, file.id.0
                        ),
                        DependencyTarget::Filename(name) => name.clone(),
                    };
                    format!("{:?}: {target}", dependency.kind)
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        12 => (
            "URL",
            file.download_url
                .as_ref()
                .map_or_else(|| "No direct download URL".into(), |url| url.to_string()),
        ),
        _ => (
            "Hashes",
            file.hashes
                .iter()
                .map(|hash| {
                    use kako_mods_lib::source::FileHash;
                    match hash {
                        FileHash::Sha1(value) => format!("SHA1 {value}"),
                        FileHash::Sha512(value) => format!("SHA512 {value}"),
                        FileHash::Md5(value) => format!("MD5 {value}"),
                    }
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
    }
}

fn channel(release: ReleaseType) -> &'static str {
    match release {
        ReleaseType::Release => "release",
        ReleaseType::Beta => "beta",
        ReleaseType::Alpha => "alpha",
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    frame: &mut Frame,
    label: &str,
    project: &ProjectRef,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
    query: &FileQuery,
    current: usize,
    count: usize,
    browser: &mut Browser,
) {
    browser.table_area = Rect::default();
    browser.detail_area = Rect::default();
    browser.pinned_area = Rect::default();
    let area = frame.area();
    if area.width < 28 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal · Esc/q back").style(style(Role::Muted)),
            area,
        );
        return;
    }
    let layout = BrowserLayout::new(
        area,
        browser.full_details,
        browser.focus == Focus::Details,
        0,
        if pinned.is_some() { 4 } else { 0 },
        8,
    );
    let total = page
        .total
        .map_or_else(|| "?".into(), |total| total.to_string());
    let range = if page.files.is_empty() {
        "0".into()
    } else {
        format!(
            "{}–{}",
            page.offset + 1,
            page.offset + page.files.len() as u64
        )
    };
    frame.render_widget(
        Paragraph::new(short_line(
            vec![
                Span::styled(format!("Files · {}", text(label)), style(Role::Primary)),
                Span::styled(format!(" · {range} / {total} · "), style(Role::Muted)),
                Span::styled(text(&project.source.0), widgets::source(&project.source.0)),
                Span::styled(format!(" / {}", text(&project.id.0)), style(Role::Muted)),
            ],
            layout.title.width as usize,
        )),
        layout.title,
    );
    if !browser.full_details {
        if let Some(file) = pinned {
            browser.pinned_area = layout.pinned;
            let focused = browser.focus == Focus::Files && browser.pinned;
            let block = panel(" Selected ", focused).padding(Padding::horizontal(1));
            let inner = block.inner(layout.pinned);
            let mut summary = vec![
                Span::styled(text(&file.name), style(Role::Primary)),
                Span::styled(
                    format!(" · Minecraft {} · ", text(&file.game_versions.join(", "))),
                    style(Role::Secondary),
                ),
            ];
            summary.extend(loader_spans(file));
            let lines = vec![
                short_line(summary, inner.width as usize),
                short_line(version_line(file), inner.width as usize),
            ];
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .style(browser::selection(focused)),
                layout.pinned,
            );
        }
        draw_table(frame, layout.table, page, pinned, browser);
    }
    if layout.details.height > 0 {
        draw_details(frame, layout.details, page, pinned, browser);
    }
    let mut context = format!(
        "Project {} / {} · offset {}",
        current + 1,
        count,
        page.offset
    );
    if let Some(version) = &query.game_version {
        context.push_str(&format!(" · Minecraft {}", text(version)));
    }
    if !query.loaders.is_empty() {
        context.push_str(&format!(" · Loaders {}", text(&query.loaders.join(", "))));
    }
    if let Some(release) = query.release_type {
        context.push_str(&format!(" · {}", channel(release)));
    }
    let (status, role) = browser
        .status
        .as_ref()
        .map(|(message, role)| (text(message), *role))
        .unwrap_or((context, Role::Muted));
    let help = if browser.focus == Focus::Details {
        "↑/↓ Field · c Copy · Enter Full Details · Tab Files"
    } else {
        "↑/↓ Move · PgUp/PgDn Scroll · ←/→ / n/p Page · Home/End"
    };
    let navigation = if count > 1 {
        "Tab Switch Card · [/] Project · Enter Details · q / Esc Back"
    } else {
        "Tab Switch Card · Enter Details · q / Esc Back"
    };
    frame.render_widget(
        Paragraph::new(vec![
            short_line(
                vec![Span::styled(status, style(role))],
                layout.footer.width as usize,
            ),
            short_line(
                vec![Span::styled(help, style(Role::Normal))],
                layout.footer.width as usize,
            ),
            short_line(
                vec![Span::styled(navigation, style(Role::Muted))],
                layout.footer.width as usize,
            ),
        ]),
        layout.footer,
    );
}

fn name_style(file: &ProjectFile, pinned: Option<&ProjectFile>) -> Style {
    if pinned.is_none_or(|selected| selected.reference == file.reference) {
        style(Role::Primary)
    } else {
        style(Role::Secondary).add_modifier(Modifier::BOLD)
    }
}

fn loader_spans(file: &ProjectFile) -> Vec<Span<'static>> {
    file.loaders
        .iter()
        .enumerate()
        .flat_map(|(index, loader)| {
            [
                Span::raw(if index == 0 { "" } else { " · " }),
                Span::styled(text(loader), widgets::loader(loader)),
            ]
        })
        .collect()
}

fn version_line(file: &ProjectFile) -> Vec<Span<'static>> {
    vec![
        Span::styled("Version: ", style(Role::Muted)),
        Span::styled(
            text(file.version.as_deref().unwrap_or("unknown version")),
            style(Role::Secondary),
        ),
        Span::styled(
            format!(" · Release: {}", channel(file.release_type)),
            style(Role::Muted),
        ),
    ]
}

fn draw_table(
    frame: &mut Frame,
    area: Rect,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
    browser: &mut Browser,
) {
    browser.table_area = area;
    let focused = browser.focus == Focus::Files && !browser.pinned;
    let block = panel(" Files ", focused);
    let inner = block.inner(area);
    let labels = &["Index", "Name", "Minecraft", "Loaders"];
    let weights = &[8, 40, 24, 20];
    let sizing = TableColumns::new(inner.width, weights, 0);
    let header = Row::new(
        labels
            .iter()
            .zip(["", "Version", "Release", ""])
            .zip(&sizing.areas)
            .map(|((&label, secondary), area)| {
                Cell::from(Text::from(vec![
                    short_line(vec![Span::raw(label)], area.width as usize),
                    short_line(vec![Span::raw(secondary)], area.width as usize),
                ]))
            })
            .collect::<Vec<_>>(),
    )
    .height(2)
    .bottom_margin(1)
    .style(style(Role::Muted));
    let rows = page
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let cells = labels
                .iter()
                .zip(&sizing.areas)
                .map(|(&label, area)| {
                    let (first, second) = match label {
                        "Index" => (
                            vec![Span::styled(
                                (page.offset + index as u64 + 1).to_string(),
                                style(Role::Muted),
                            )],
                            vec![],
                        ),
                        "Name" => (
                            vec![Span::styled(text(&file.name), name_style(file, pinned))],
                            vec![Span::styled(
                                text(file.version.as_deref().unwrap_or("unknown version")),
                                style(Role::Secondary),
                            )],
                        ),
                        "Minecraft" => (
                            vec![Span::styled(
                                text(&file.game_versions.join(", ")),
                                style(Role::Secondary),
                            )],
                            vec![Span::styled(channel(file.release_type), style(Role::Muted))],
                        ),
                        _ => (loader_spans(file), vec![]),
                    };
                    Cell::from(Text::from(vec![
                        short_line(first, area.width as usize),
                        short_line(second, area.width as usize),
                    ]))
                })
                .collect::<Vec<_>>();
            Row::new(cells).height(2)
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        sizing.table(rows, header, block, focused),
        area,
        &mut browser.table,
    );
    if page.files.is_empty() {
        let message = if pinned.is_some() {
            "No matching files on this page."
        } else {
            "No matching files."
        };
        frame.render_widget(
            Paragraph::new(message).style(style(Role::Muted)),
            Rect {
                y: inner.y.saturating_add(TABLE_HEADER_HEIGHT),
                height: inner.height.saturating_sub(TABLE_HEADER_HEIGHT),
                ..inner
            },
        );
    }
    browser.visible_rows = (inner.height.saturating_sub(TABLE_HEADER_HEIGHT) / 2).max(1) as usize;
}

fn draw_details(
    frame: &mut Frame,
    area: Rect,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
    browser: &mut Browser,
) {
    browser.detail_area = area;
    let block = panel(
        if browser.pinned {
            " Details · Selected "
        } else {
            " Details "
        },
        browser.focus == Focus::Details,
    );
    let inner = block.inner(area);
    let Some(file) = browser.file(page, pinned) else {
        frame.render_widget(
            Paragraph::new("Select a file to inspect its details.")
                .style(style(Role::Muted))
                .block(block),
            area,
        );
        return;
    };
    let items = (0..FIELD_COUNT)
        .map(|field| {
            let (label, value) = field_value(file, field);
            let paint = match field {
                0 => name_style(file, pinned),
                1 | 6 => style(Role::Secondary),
                3 => widgets::source(&file.reference.project.source.0),
                _ => style(Role::Muted),
            };
            ListItem::new(short_line(
                vec![
                    Span::styled(format!("{label}: "), style(Role::Muted)),
                    Span::styled(text(&value), paint),
                ],
                inner.width.saturating_sub(2) as usize,
            ))
        })
        .collect();
    browser::draw_fields(
        frame,
        area,
        block,
        items,
        browser.field,
        &mut browser.detail_offset,
        browser.focus == Focus::Details,
    );
}
