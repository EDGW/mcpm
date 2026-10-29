//! File and field navigation; provider pagination is separate from viewport movement.

use super::{Browser, Focus, view};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use mcpm_backend::source::{FilePage, ProjectFile};
use ratatui::layout::Position;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Quit,
    Interrupt,
    Project(bool),
    Page(u64),
    Redraw,
}

pub(super) fn handle(
    browser: &mut Browser,
    event: Event,
    page: &FilePage,
    pinned: Option<&ProjectFile>,
    limit: u32,
) -> Action {
    let count = page.files.len();
    let key = match event {
        Event::Mouse(mouse) => {
            let position = Position::new(mouse.column, mouse.row);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if browser.detail_area.contains(position) {
                        browser.focus = Focus::Details;
                        browser.field = (browser.detail_offset
                            + usize::from(mouse.row.saturating_sub(browser.detail_area.y + 1)))
                        .min(view::FIELD_COUNT - 1);
                    } else if browser.pinned_area.contains(position) && pinned.is_some() {
                        browser.focus = Focus::Files;
                        browser.select_pinned();
                    } else if browser.table_area.contains(position)
                        && mouse.row
                            >= browser
                                .table_area
                                .y
                                .saturating_add(1 + view::TABLE_HEADER_HEIGHT)
                        && mouse.row < browser.table_area.bottom().saturating_sub(1)
                    {
                        browser.focus = Focus::Files;
                        browser.select(
                            browser.table.offset()
                                + usize::from(
                                    (mouse.row
                                        - browser.table_area.y
                                        - 1
                                        - view::TABLE_HEADER_HEIGHT)
                                        / 2,
                                ),
                            count,
                        );
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let focus = browser.focus;
                    browser.focus = if browser.detail_area.contains(position) {
                        Focus::Details
                    } else if browser.table_area.contains(position)
                        || browser.pinned_area.contains(position)
                    {
                        Focus::Files
                    } else {
                        return Action::Redraw;
                    };
                    browser.move_by(
                        if mouse.kind == MouseEventKind::ScrollUp {
                            -1
                        } else {
                            1
                        },
                        count,
                        pinned.is_some(),
                    );
                    browser.focus = focus;
                }
                _ => {}
            }
            return Action::Redraw;
        }
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => key,
        _ => return Action::Redraw,
    };
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Interrupt;
    }
    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Esc if browser.full_details => {
            browser.full_details = false;
            browser.focus = Focus::Files;
        }
        KeyCode::Esc => return Action::Quit,
        KeyCode::Tab | KeyCode::BackTab => {
            browser.full_details = false;
            browser.focus = if browser.focus == Focus::Files {
                Focus::Details
            } else {
                Focus::Files
            };
        }
        KeyCode::Enter => {
            browser.full_details = !browser.full_details;
            browser.focus = if browser.full_details {
                Focus::Details
            } else {
                Focus::Files
            };
        }
        KeyCode::Char('c') if browser.focus == Focus::Details => browser.copy_field(page, pinned),
        KeyCode::Char('[' | ']') => return Action::Project(key.code == KeyCode::Char('[')),
        KeyCode::Left | KeyCode::Char('p') if browser.focus == Focus::Files && page.offset > 0 => {
            return Action::Page(page.offset.saturating_sub(u64::from(limit)));
        }
        KeyCode::Right | KeyCode::Char('n') if browser.focus == Focus::Files => {
            let offset = page.offset.saturating_add(u64::from(limit));
            if !page.files.is_empty() && page.total.is_none_or(|total| offset < total) {
                return Action::Page(offset);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => browser.move_by(-1, count, pinned.is_some()),
        KeyCode::Down | KeyCode::Char('j') => browser.move_by(1, count, pinned.is_some()),
        KeyCode::PageUp | KeyCode::PageDown => {
            let amount = if browser.focus == Focus::Details {
                browser.detail_area.height.saturating_sub(2).max(1) as isize
            } else {
                browser.visible_rows as isize
            };
            browser.move_by(
                if key.code == KeyCode::PageUp {
                    -amount
                } else {
                    amount
                },
                count,
                pinned.is_some(),
            );
        }
        KeyCode::Home if browser.focus == Focus::Details => browser.field = 0,
        KeyCode::End if browser.focus == Focus::Details => browser.field = view::FIELD_COUNT - 1,
        KeyCode::Home if pinned.is_some() => browser.select_pinned(),
        KeyCode::Home => browser.select(0, count),
        KeyCode::End => {
            if count == 0 && pinned.is_some() {
                browser.select_pinned();
            } else {
                browser.select(count.saturating_sub(1), count);
            }
        }
        _ => {}
    }
    Action::Redraw
}
