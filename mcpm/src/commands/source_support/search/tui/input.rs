//! Focus-aware search editing, result navigation, and bounded details scrolling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kako_mods_lib::source::SearchPage;

use super::{Browser, Focus};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Quit,
    Search,
    OpenFiles,
    Page(u64),
    Redraw,
}

impl Browser {
    pub(super) fn handle(
        &mut self,
        key: KeyEvent,
        committed: &str,
        page: &SearchPage,
        limit: u32,
    ) -> Action {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = match (self.focus, key.code == KeyCode::BackTab) {
                (Focus::Search, false) | (Focus::Details, true) => Focus::Results,
                (Focus::Results, false) | (Focus::Search, true) => Focus::Details,
                _ => Focus::Search,
            };
            return Action::Redraw;
        }
        if self.focus == Focus::Search {
            return self.edit(key, committed);
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Char('/') => self.focus = Focus::Search,
            KeyCode::Enter => return Action::OpenFiles,
            _ if self.focus == Focus::Details => {
                self.detail_scroll = match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.detail_scroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => self.detail_scroll.saturating_add(1),
                    KeyCode::PageUp => self.detail_scroll.saturating_sub(5),
                    KeyCode::PageDown => self.detail_scroll.saturating_add(5),
                    KeyCode::Home => 0,
                    KeyCode::End => self.detail_limit,
                    _ => self.detail_scroll,
                }
                .min(self.detail_limit);
            }
            KeyCode::Up | KeyCode::Char('k') if self.selected > 0 => {
                self.selected -= 1;
                self.detail_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if self.selected + 1 < page.projects.len() => {
                self.selected += 1;
                self.detail_scroll = 0;
            }
            KeyCode::Home => {
                self.selected = 0;
                self.detail_scroll = 0;
            }
            KeyCode::End => {
                self.selected = page.projects.len().saturating_sub(1);
                self.detail_scroll = 0;
            }
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

    fn edit(&mut self, key: KeyEvent, committed: &str) -> Action {
        match key.code {
            KeyCode::Enter => return Action::Search,
            KeyCode::Esc => {
                self.draft = committed.to_owned();
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
