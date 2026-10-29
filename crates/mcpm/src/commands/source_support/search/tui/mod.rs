//! Search browsing state and cancellable requests; rendering and keyboard input stay separate.

use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use mcpm_backend::source::{FileQuery, SearchPage, SearchQuery, Source};
use tokio::runtime::Runtime;

use super::super::request;
use super::expression::{Expression, ParseError};
use crate::{
    theme::Role,
    widgets::browser::{self, Screen},
};
use completion::Completion;
use input::Action;
use ratatui::{layout::Rect, widgets::TableState};

mod completion;
mod input;
mod view;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Search,
    Results,
    Details,
}

/// Search browser state.
///
/// # Implementations
/// - [`Self::new`], [`Self::complete`]: browser state and completion.
/// - [`Self::handle`]: keyboard handling in `input`.
/// - [`std::fmt::Debug::fmt`]: concise state debugging.
struct Browser {
    draft: String,
    committed: String,
    parse_error: Option<ParseError>,
    cursor: usize,
    focus: Focus,
    table: TableState,
    field: usize,
    detail_offset: usize,
    detail_row: usize,
    detail_fields: Vec<usize>,
    full_details: bool,
    search_area: Rect,
    table_area: Rect,
    detail_area: Rect,
    row_height: u16,
    clipboard: Option<arboard::Clipboard>,
    status: Option<crate::widgets::Status>,
    completion: Completion,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("draft", &self.draft)
            .field("committed", &self.committed)
            .field("parse_error", &self.parse_error)
            .field("cursor", &self.cursor)
            .field("focus", &self.focus)
            .field("table", &self.table)
            .field("field", &self.field)
            .field("detail_offset", &self.detail_offset)
            .field("detail_row", &self.detail_row)
            .field("detail_fields", &self.detail_fields)
            .field("full_details", &self.full_details)
            .field("search_area", &self.search_area)
            .field("table_area", &self.table_area)
            .field("detail_area", &self.detail_area)
            .field("row_height", &self.row_height)
            .field("status", &self.status)
            .field("completion", &self.completion)
            .field("clipboard_available", &self.clipboard.is_some())
            .finish()
    }
}

impl Browser {
    fn new(query: &str) -> Self {
        Self {
            draft: query.to_owned(),
            committed: query.to_owned(),
            parse_error: None,
            cursor: query.len(),
            focus: Focus::Results,
            table: TableState::default(),
            field: 0,
            detail_offset: 0,
            detail_row: 0,
            detail_fields: (0..view::FIELD_COUNT).collect(),
            full_details: false,
            search_area: Rect::default(),
            table_area: Rect::default(),
            detail_area: Rect::default(),
            row_height: 2,
            clipboard: None,
            status: None,
            completion: Completion::default(),
        }
    }

    fn select(&mut self, index: usize, count: usize) {
        self.table
            .select((count > 0).then_some(index.min(count.saturating_sub(1))));
        self.detail_offset = 0;
    }

    fn refresh_completion(&mut self) {
        self.completion
            .refresh(&self.draft, self.cursor, self.focus == Focus::Search);
    }

    fn complete(&mut self) -> bool {
        let accepted = self.completion.accept(&mut self.draft, &mut self.cursor);
        if accepted {
            self.parse_error = None;
            self.status = None;
        }
        accepted
    }

    fn project<'a>(&self, page: &'a SearchPage) -> Option<&'a mcpm_backend::source::ProjectInfo> {
        page.projects.get(self.table.selected()?)
    }

    fn copy_field(&mut self, page: &SearchPage) {
        if let Some(project) = self.project(page) {
            let (label, value) = view::field_value(project, self.field);
            self.status = Some(browser::copy_field(&mut self.clipboard, value, label));
        }
    }

    fn select_detail_row(&mut self, row: usize) {
        self.detail_row = row.min(self.detail_fields.len().saturating_sub(1));
        self.field = self
            .detail_fields
            .get(self.detail_row)
            .copied()
            .unwrap_or(0);
    }

    fn prepare(&mut self, defaults: &SearchQuery) -> Result<SearchQuery, ParseError> {
        match Expression::parse(&self.draft) {
            Ok(expression) => {
                self.parse_error = None;
                Ok(expression.apply(defaults))
            }
            Err(error) => {
                self.cursor = self
                    .draft
                    .char_indices()
                    .nth(error.column.saturating_sub(1))
                    .map_or(self.draft.len(), |(index, _)| index);
                self.parse_error = Some(error.clone());
                Err(error)
            }
        }
    }
}

pub(super) fn run(
    source: &dyn Source,
    runtime: &Runtime,
    defaults: SearchQuery,
    expression: String,
    mut query: SearchQuery,
    mut page: SearchPage,
) -> Result<()> {
    let mut screen = Screen::new().context("failed to open search results")?;
    let mut browser = Browser::new(&expression);
    let mut held_page_key = None;
    let mut versions_request = None;
    loop {
        browser.refresh_completion();
        if browser.completion.needs_versions() {
            browser.completion.loading();
            versions_request = Some(source.game_versions());
        }
        if let Some(pending) = versions_request.as_mut() {
            // Drive the borrowed request without spawning a task or blocking typing.
            let result = runtime.block_on(async {
                tokio::select! {
                    result = pending => Some(result),
                    _ = tokio::time::sleep(Duration::from_millis(30)) => None,
                }
            });
            if let Some(result) = result {
                versions_request = None;
                browser.completion.finish_versions(result);
                browser.refresh_completion();
            }
        }
        view::render(
            &mut screen,
            source.id().0.as_str(),
            &query,
            &page,
            &mut browser,
            "",
        )?;
        if versions_request.is_some() && !event::poll(Duration::ZERO)? {
            continue;
        }
        // Legacy terminals report auto-repeat as fresh presses and provide no
        // release event. A quiet interval marks the end of that repeat stream.
        if held_page_key.is_some() && !event::poll(Duration::from_millis(180))? {
            held_page_key = None;
        }
        let next = event::read().context("failed to read search input")?;
        if let Event::Key(key) = &next {
            if held_page_key == Some(key.code) {
                if key.kind == KeyEventKind::Release {
                    held_page_key = None;
                }
                continue;
            }
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            held_page_key = None;
        }
        let action = input::handle(&mut browser, next.clone(), &page, query.limit);
        let next_offset = match action {
            Action::Quit => return Ok(()),
            Action::Complete => {
                held_page_key = if let Event::Key(key) = next {
                    Some(key.code)
                } else {
                    None
                };
                None
            }
            Action::Search => {
                held_page_key = Some(KeyCode::Enter);
                let pending = match browser.prepare(&defaults) {
                    Ok(query) => query,
                    Err(error) => {
                        browser.status = Some(crate::widgets::Status::error(error.into()));
                        continue;
                    }
                };
                match request::wait(
                    runtime,
                    source.search(&pending),
                    "Searching projects...",
                    |loading| {
                        view::render(
                            &mut screen,
                            source.id().0.as_str(),
                            &query,
                            &page,
                            &mut browser,
                            loading,
                        )
                    },
                ) {
                    Ok(Some(next)) => {
                        query = pending;
                        browser.committed = browser.draft.clone();
                        page = next;
                        browser.table = TableState::default();
                        browser.select(0, page.projects.len());
                        browser.focus = Focus::Results;
                        browser.status = None;
                    }
                    Ok(None) => browser.status = Some(("Cancelled.".into(), Role::Muted).into()),
                    Err(error) if request::is_interrupted(&error) => return Err(error),
                    Err(error) => browser.status = Some(crate::widgets::Status::error(error)),
                }
                None
            }
            Action::OpenFiles => {
                if let Some(project) = browser.project(&page) {
                    let files_query = FileQuery {
                        game_versions: query.game_versions.clone(),
                        loaders: query.loaders.clone(),
                        ..Default::default()
                    };
                    view::render(
                        &mut screen,
                        source.id().0.as_str(),
                        &query,
                        &page,
                        &mut browser,
                        "Loading files...",
                    )?;
                    match request::wait(
                        runtime,
                        source.list_files(&project.reference, &files_query),
                        "Fetching files...",
                        |loading| {
                            view::render(
                                &mut screen,
                                source.id().0.as_str(),
                                &query,
                                &page,
                                &mut browser,
                                loading,
                            )
                        },
                    ) {
                        Ok(Some(files)) => {
                            // Only one screen owns raw mode and the alternate buffer.
                            // Reopen search after the file browser has restored them.
                            drop(screen);
                            let result = super::super::files::tui::run(
                                source,
                                runtime,
                                files_query,
                                vec![(project.name.clone(), project.reference.clone(), files)],
                                None,
                            );
                            screen = Screen::new().context("failed to restore search results")?;
                            if result.as_ref().err().is_some_and(request::is_interrupted) {
                                return result;
                            }
                            browser.status = result.err().map(crate::widgets::Status::error);
                            held_page_key = Some(KeyCode::Enter);
                        }
                        Ok(None) => {
                            browser.status = Some(("Cancelled.".into(), Role::Muted).into())
                        }
                        Err(error) if request::is_interrupted(&error) => return Err(error),
                        Err(error) => browser.status = Some(crate::widgets::Status::error(error)),
                    }
                }
                None
            }
            Action::Page(offset) => Some(offset),
            Action::Redraw => None,
        };
        if let Some(offset) = next_offset {
            held_page_key = if let Event::Key(key) = next {
                Some(key.code)
            } else {
                None
            };
            view::render(
                &mut screen,
                source.id().0.as_str(),
                &query,
                &page,
                &mut browser,
                "Loading...",
            )?;
            let previous = query.offset;
            let backwards = offset < page.offset;
            query.offset = offset;
            match request::wait(
                runtime,
                source.search(&query),
                "Searching projects...",
                |loading| {
                    view::render(
                        &mut screen,
                        source.id().0.as_str(),
                        &query,
                        &page,
                        &mut browser,
                        loading,
                    )
                },
            ) {
                Ok(Some(next)) => {
                    if next.projects.is_empty() {
                        query.offset = previous;
                        browser.status = Some(("No more results.".into(), Role::Muted).into());
                        continue;
                    }
                    page = next;
                    browser.table = TableState::default();
                    browser.select(
                        if backwards {
                            page.projects.len() - 1
                        } else {
                            0
                        },
                        page.projects.len(),
                    );
                    browser.status = None;
                }
                Ok(None) => {
                    query.offset = previous;
                    browser.status = Some(("Cancelled.".into(), Role::Muted).into());
                }
                Err(error) if request::is_interrupted(&error) => return Err(error),
                Err(error) => {
                    query.offset = previous;
                    browser.status = Some(crate::widgets::Status::error(error));
                }
            }
        }
    }
}
