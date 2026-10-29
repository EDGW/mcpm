//! Search editing and project/field navigation over shared browser widget state.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use mcpm_backend::source::SearchPage;
use ratatui::layout::Position;

use super::{Browser, Focus, view};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Quit,
    Search,
    Complete,
    OpenFiles,
    Page(u64),
    Redraw,
}

pub(super) fn handle(browser: &mut Browser, event: Event, page: &SearchPage, limit: u32) -> Action {
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            browser.handle(key, page, limit)
        }
        Event::Mouse(mouse) => {
            let position = Position::new(mouse.column, mouse.row);
            if browser.completion.area.contains(position) {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left)
                        if browser.completion.click(mouse.row) =>
                    {
                        if browser.complete() {
                            return Action::Complete;
                        }
                    }
                    MouseEventKind::ScrollUp => browser.completion.move_by(-1),
                    MouseEventKind::ScrollDown => browser.completion.move_by(1),
                    _ => {}
                }
                return Action::Redraw;
            }
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if browser.search_area.contains(position) {
                        browser.focus = Focus::Search;
                        browser.full_details = false;
                    } else if browser.detail_area.contains(position) {
                        browser.focus = Focus::Details;
                        browser.select_detail_row(
                            browser.detail_offset
                                + usize::from(mouse.row.saturating_sub(browser.detail_area.y + 1)),
                        );
                    } else if browser.table_area.contains(position)
                        && mouse.row >= browser.table_area.y + 1 + view::TABLE_HEADER_HEIGHT
                        && mouse.row < browser.table_area.bottom().saturating_sub(1)
                    {
                        browser.focus = Focus::Results;
                        let row = browser.table.offset()
                            + usize::from(
                                (mouse.row - browser.table_area.y - 1 - view::TABLE_HEADER_HEIGHT)
                                    / browser.row_height,
                            );
                        browser.select(row, page.projects.len());
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let amount = if mouse.kind == MouseEventKind::ScrollUp {
                        -1
                    } else {
                        1
                    };
                    if browser.detail_area.contains(position) {
                        browser.select_detail_row(browser.detail_row.saturating_add_signed(amount));
                    } else if browser.table_area.contains(position) {
                        browser.select(
                            browser
                                .table
                                .selected()
                                .unwrap_or(0)
                                .saturating_add_signed(amount),
                            page.projects.len(),
                        );
                    }
                }
                _ => {}
            }
            Action::Redraw
        }
        _ => Action::Redraw,
    }
}

impl Browser {
    pub(super) fn handle(&mut self, key: KeyEvent, page: &SearchPage, limit: u32) -> Action {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }
        self.refresh_completion();
        if self.focus == Focus::Search && self.completion.open() {
            match key.code {
                KeyCode::Up | KeyCode::Down => {
                    self.completion
                        .move_by(if key.code == KeyCode::Up { -1 } else { 1 });
                    return Action::Redraw;
                }
                KeyCode::PageUp | KeyCode::PageDown => {
                    self.completion
                        .move_by(if key.code == KeyCode::PageUp { -6 } else { 6 });
                    return Action::Redraw;
                }
                KeyCode::Enter | KeyCode::Tab if self.complete() => return Action::Complete,
                KeyCode::Esc => {
                    self.completion.hide(&self.draft, self.cursor);
                    return Action::Redraw;
                }
                KeyCode::Char('r')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.completion.retry_versions() =>
                {
                    return Action::Redraw;
                }
                _ => {}
            }
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.full_details = false;
            self.focus = match (self.focus, key.code == KeyCode::BackTab) {
                (Focus::Search, false) | (Focus::Details, true) => Focus::Results,
                (Focus::Results, false) | (Focus::Search, true) => Focus::Details,
                _ => Focus::Search,
            };
            return Action::Redraw;
        }
        if self.focus == Focus::Search {
            let action = self.edit(key);
            self.refresh_completion();
            return action;
        }
        let selected = self.table.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc if self.full_details => {
                self.full_details = false;
                self.focus = Focus::Results;
            }
            KeyCode::Esc => return Action::Quit,
            KeyCode::Char('/') => {
                self.full_details = false;
                self.focus = Focus::Search;
            }
            KeyCode::Enter if self.focus == Focus::Results => return Action::OpenFiles,
            KeyCode::Enter => self.full_details = !self.full_details,
            KeyCode::Char('c') if self.focus == Focus::Details => self.copy_field(page),
            _ if self.focus == Focus::Details => {
                let amount = self.detail_area.height.saturating_sub(2).max(1) as usize;
                let row = match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.detail_row.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => self.detail_row.saturating_add(1),
                    KeyCode::PageUp => self.detail_row.saturating_sub(amount),
                    KeyCode::PageDown => self.detail_row.saturating_add(amount),
                    KeyCode::Home => 0,
                    KeyCode::End => self.detail_fields.len().saturating_sub(1),
                    _ => self.detail_row,
                };
                self.select_detail_row(row);
            }
            KeyCode::Up | KeyCode::Char('k') if selected > 0 => {
                self.select(selected - 1, page.projects.len())
            }
            KeyCode::Down | KeyCode::Char('j') if selected + 1 < page.projects.len() => {
                self.select(selected + 1, page.projects.len())
            }
            KeyCode::Home => self.select(0, page.projects.len()),
            KeyCode::End => self.select(page.projects.len().saturating_sub(1), page.projects.len()),
            KeyCode::Right
            | KeyCode::PageDown
            | KeyCode::Char('n')
            | KeyCode::Down
            | KeyCode::Char('j') => {
                let next = page.offset.saturating_add(u64::from(limit));
                if !page.projects.is_empty() && page.total.is_none_or(|total| next < total) {
                    return Action::Page(next);
                }
            }
            KeyCode::Left
            | KeyCode::PageUp
            | KeyCode::Char('p')
            | KeyCode::Up
            | KeyCode::Char('k')
                if page.offset > 0 =>
            {
                return Action::Page(page.offset.saturating_sub(u64::from(limit)));
            }
            _ => {}
        }
        Action::Redraw
    }

    fn edit(&mut self, key: KeyEvent) -> Action {
        self.parse_error = None;
        match key.code {
            KeyCode::Enter => return Action::Search,
            KeyCode::Esc => {
                self.draft = self.committed.clone();
                self.cursor = self.draft.len();
                self.focus = Focus::Results;
            }
            KeyCode::Left if self.cursor > 0 => self.cursor = self.previous_char(),
            KeyCode::Right if self.cursor < self.draft.len() => {
                self.cursor += self.draft[self.cursor..].chars().next().unwrap().len_utf8();
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.draft.len(),
            KeyCode::Backspace if self.cursor > 0 => {
                let previous = self.previous_char();
                self.draft.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete if self.cursor < self.draft.len() => {
                self.draft.remove(self.cursor);
            }
            KeyCode::Char(ch)
                if !ch.is_control()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.draft.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
            }
            _ => {}
        }
        Action::Redraw
    }

    fn previous_char(&self) -> usize {
        self.draft[..self.cursor]
            .char_indices()
            .next_back()
            .unwrap()
            .0
    }
}
