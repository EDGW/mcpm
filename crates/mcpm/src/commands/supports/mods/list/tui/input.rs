//! Ordered keyboard and wheel handling with immediate scroll boundary clamping.

use super::{Browser, DETAIL_FIELDS, Focus, Summary};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};

pub(super) enum Update {
    Quit,
    OpenFiles(mcpm_backend::source::FileRef),
    Redraw,
    Ignore,
}
impl From<bool> for Update {
    fn from(changed: bool) -> Self {
        if changed { Self::Redraw } else { Self::Ignore }
    }
}

pub(super) fn handle(
    browser: &mut Browser,
    summary: &Summary<'_>,
    event: Event,
    page: usize,
) -> Update {
    let key = match event {
        Event::Resize(_, _) => return Update::Redraw,
        Event::Mouse(mouse) => {
            let position = ratatui::layout::Position::new(mouse.column, mouse.row);
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if browser.filter_area.contains(position) {
                    browser.open_filter();
                    return Update::Redraw;
                }
                if browser.detail_area.contains(position) {
                    browser.focus = Focus::Details;
                    if mouse.row > browser.detail_area.y
                        && mouse.row < browser.detail_area.bottom().saturating_sub(1)
                        && let Some(field) =
                            browser.detail_field_at(mouse.row - browser.detail_area.y - 1)
                    {
                        browser.field = field;
                    }
                    return Update::Redraw;
                }
                if browser.table_area.contains(position)
                    && mouse.row >= browser.table_area.y.saturating_add(3)
                    && mouse.row < browser.table_area.bottom().saturating_sub(1)
                {
                    browser.focus = Focus::Mods;
                    let position = browser.table.offset()
                        + usize::from((mouse.row - browser.table_area.y - 3) / 2);
                    browser.select(position);
                    if mouse.column == browser.table_area.x.saturating_add(1)
                        && (mouse.row - browser.table_area.y - 3).is_multiple_of(2)
                    {
                        browser.toggle();
                    }
                    return Update::Redraw;
                }
                return Update::Ignore;
            }
            let amount = match mouse.kind {
                MouseEventKind::ScrollUp => -1,
                MouseEventKind::ScrollDown => 1,
                _ => return Update::Ignore,
            };
            if browser.detail_area.contains(position) {
                let focus = browser.focus;
                browser.focus = Focus::Details;
                let update = browser.move_by(amount).into();
                browser.focus = focus;
                return update;
            }
            if browser.table_area.contains(position) {
                let focus = browser.focus;
                browser.focus = Focus::Mods;
                let update = browser.move_by(amount).into();
                browser.focus = focus;
                return update;
            }
            return Update::Ignore;
        }
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => key,
        _ => return Update::Ignore,
    };
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Update::Quit;
    }
    if browser.focus == Focus::Filter {
        return filter_input(browser, key.code, key.modifiers);
    }
    match key.code {
        KeyCode::Char('q') => return Update::Quit,
        KeyCode::Esc if browser.details || browser.ignored => {
            browser.details = false;
            browser.focus = Focus::Mods;
            browser.ignored = false;
            browser.scroll = 0;
        }
        KeyCode::Esc => return Update::Quit,
        KeyCode::Char('f' | 'F') => browser.open_filter(),
        KeyCode::Char('e') => browser.expand_all(),
        KeyCode::Tab | KeyCode::BackTab => browser.cycle_focus(key.code == KeyCode::BackTab),
        KeyCode::Right if browser.focus == Focus::Mods => return browser.expand(true).into(),
        KeyCode::Left if browser.focus == Focus::Mods => return browser.expand(false).into(),
        KeyCode::Char(' ') if browser.focus == Focus::Mods => return browser.toggle().into(),
        KeyCode::Char('c') if browser.focus == Focus::Details && !browser.ignored => {
            browser.copy_field()
        }
        KeyCode::Char('i') if summary.ignored > 0 => {
            browser.ignored = !browser.ignored;
            browser.focus = if browser.ignored {
                Focus::Details
            } else {
                Focus::Mods
            };
            browser.details = false;
            browser.scroll = 0;
        }
        KeyCode::Enter if browser.selected_source_file().is_some() => {
            return Update::OpenFiles(browser.selected_source_file().unwrap().clone());
        }
        KeyCode::Enter => {
            browser.details = !browser.details;
            browser.focus = if browser.details {
                Focus::Details
            } else {
                Focus::Mods
            };
            browser.ignored = false;
            browser.scroll = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => return browser.move_by(-1).into(),
        KeyCode::Down | KeyCode::Char('j') => return browser.move_by(1).into(),
        KeyCode::PageUp => return browser.move_by(-(page as isize)).into(),
        KeyCode::PageDown => return browser.move_by(page as isize).into(),
        KeyCode::Home if browser.ignored => browser.scroll = 0,
        KeyCode::End if browser.ignored => browser.scroll = browser.scroll_limit,
        KeyCode::Home if browser.focus == Focus::Details => browser.field = 0,
        KeyCode::End if browser.focus == Focus::Details => browser.field = DETAIL_FIELDS.len() - 1,
        KeyCode::Home => return browser.select(0).into(),
        KeyCode::End => {
            return browser
                .select(browser.matches.len().saturating_sub(1))
                .into();
        }
        KeyCode::Char('r') => {
            browser.query.clear();
            browser.query_cursor = 0;
            browser.filter();
        }
        _ => return Update::Ignore,
    }
    Update::Redraw
}

/// Filter typing has its own key map, so ordinary command letters remain text.
fn filter_input(browser: &mut Browser, key: KeyCode, modifiers: KeyModifiers) -> Update {
    match key {
        KeyCode::Esc | KeyCode::Char('Q') => browser.close_filter(),
        KeyCode::Tab | KeyCode::BackTab => browser.cycle_focus(key == KeyCode::BackTab),
        KeyCode::Enter => browser.focus = Focus::Mods,
        KeyCode::Left if browser.query_cursor > 0 => {
            browser.query_cursor = browser.query[..browser.query_cursor]
                .char_indices()
                .next_back()
                .unwrap()
                .0;
        }
        KeyCode::Right if browser.query_cursor < browser.query.len() => {
            browser.query_cursor += browser.query[browser.query_cursor..]
                .chars()
                .next()
                .unwrap()
                .len_utf8();
        }
        KeyCode::Home => browser.query_cursor = 0,
        KeyCode::End => browser.query_cursor = browser.query.len(),
        KeyCode::Backspace if browser.query_cursor > 0 => {
            let previous = browser.query[..browser.query_cursor]
                .char_indices()
                .next_back()
                .unwrap()
                .0;
            browser.query.drain(previous..browser.query_cursor);
            browser.query_cursor = previous;
            browser.filter();
        }
        KeyCode::Delete if browser.query_cursor < browser.query.len() => {
            browser.query.remove(browser.query_cursor);
            browser.filter();
        }
        KeyCode::Char(ch)
            if !ch.is_control()
                && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            browser.query.insert(browser.query_cursor, ch);
            browser.query_cursor += ch.len_utf8();
            browser.filter();
        }
        _ => return Update::Ignore,
    }
    Update::Redraw
}
