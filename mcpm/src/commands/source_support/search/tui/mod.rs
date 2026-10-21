//! Search browsing state and cancellable requests; rendering and keyboard input stay separate.

use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use kako_mods_lib::source::{FileQuery, SearchPage, SearchQuery, Source};
use tokio::runtime::Runtime;

use super::super::request;
use input::Action;
use view::Screen;

mod input;
mod view;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Search,
    Results,
    Details,
}

struct Browser {
    draft: String,
    cursor: usize,
    focus: Focus,
    selected: usize,
    result_offset: usize,
    detail_scroll: u16,
    detail_limit: u16,
}

impl Browser {
    fn new(query: &str) -> Self {
        Self {
            draft: query.to_owned(),
            cursor: query.len(),
            focus: Focus::Results,
            selected: 0,
            result_offset: 0,
            detail_scroll: 0,
            detail_limit: 0,
        }
    }
}

pub(super) fn run(
    source: &dyn Source,
    runtime: &Runtime,
    mut query: SearchQuery,
    mut page: SearchPage,
) -> Result<()> {
    let mut screen = Screen::new().context("failed to open search results")?;
    let mut browser = Browser::new(&query.query);
    let mut status = String::new();
    let mut held_page_key = None;
    loop {
        screen.render(source.id().0.as_str(), &query, &page, &mut browser, &status)?;
        // Legacy terminals report auto-repeat as fresh presses and provide no
        // release event. A quiet interval marks the end of that repeat stream.
        if held_page_key.is_some() && !event::poll(Duration::from_millis(180))? {
            held_page_key = None;
        }
        let Event::Key(key) = event::read().context("failed to read search input")? else {
            continue;
        };
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
        let action = browser.handle(key, &query.query, &page, query.limit);
        let next_offset = match action {
            Action::Quit => return Ok(()),
            Action::Search => {
                held_page_key = Some(KeyCode::Enter);
                let mut pending = query.clone();
                pending.query = browser.draft.clone();
                pending.offset = 0;
                match request::wait(
                    runtime,
                    source.search(&pending),
                    "Searching projects...",
                    |loading| {
                        screen.render(source.id().0.as_str(), &query, &page, &mut browser, loading)
                    },
                ) {
                    Ok(Some(next)) => {
                        query = pending;
                        page = next;
                        browser.selected = 0;
                        browser.result_offset = 0;
                        browser.detail_scroll = 0;
                        browser.focus = Focus::Results;
                        status.clear();
                    }
                    Ok(None) => status = "Cancelled.".into(),
                    Err(error) if request::is_interrupted(&error) => return Err(error),
                    Err(error) => status = format!("{error:#}"),
                }
                None
            }
            Action::OpenFiles => {
                if let Some(project) = page.projects.get(browser.selected) {
                    let files_query = FileQuery {
                        game_version: query.game_version.clone(),
                        loaders: query.loader.iter().cloned().collect(),
                        ..Default::default()
                    };
                    screen.render(
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
                            screen.render(
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
                            status = result.err().map(|e| format!("{e:#}")).unwrap_or_default();
                            held_page_key = Some(KeyCode::Enter);
                        }
                        Ok(None) => status = "Cancelled.".into(),
                        Err(error) if request::is_interrupted(&error) => return Err(error),
                        Err(error) => status = format!("{error:#}"),
                    }
                }
                None
            }
            Action::Page(offset) => Some(offset),
            Action::Redraw => None,
        };
        if let Some(offset) = next_offset {
            held_page_key = Some(key.code);
            screen.render(
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
                    screen.render(source.id().0.as_str(), &query, &page, &mut browser, loading)
                },
            ) {
                Ok(Some(next)) => {
                    if next.projects.is_empty() {
                        query.offset = previous;
                        status = "No more results.".into();
                        continue;
                    }
                    page = next;
                    browser.detail_scroll = 0;
                    browser.result_offset = 0;
                    browser.selected = if backwards {
                        page.projects.len() - 1
                    } else {
                        0
                    };
                    status.clear();
                }
                Ok(None) => {
                    query.offset = previous;
                    status = "Cancelled.".into();
                }
                Err(error) if request::is_interrupted(&error) => return Err(error),
                Err(error) => {
                    query.offset = previous;
                    status = format!("{error:#}");
                }
            }
        }
    }
}
