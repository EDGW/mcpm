//! Remote file browsing with shared list/detail widgets and cancellable pagination.

use super::super::request;
use crate::{
    display::style::text,
    theme::Role,
    widgets::browser::{self, Screen},
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use mcpm_backend::source::{FilePage, FileQuery, FileRef, ProjectFile, ProjectRef, Source};
use ratatui::{layout::Rect, widgets::TableState};
use std::{io, time::Duration};
use tokio::runtime::Runtime;

mod input;
#[cfg(test)]
mod tests;
mod view;

pub(in crate::commands::source_support) fn run(
    source: &dyn Source,
    runtime: &Runtime,
    query: FileQuery,
    projects: Vec<(String, ProjectRef, FilePage)>,
    selected_file: Option<ProjectFile>,
) -> Result<()> {
    browse(
        source,
        runtime,
        query,
        projects,
        selected_file,
        Screen::new()?,
    )
}

/// Opens an identified publication without resolving its already canonical project.
pub(in crate::commands) fn open_selected(
    runtime: &Runtime,
    reference: &FileRef,
    loaders: &[String],
) -> Result<()> {
    let source = super::super::build_source(&reference.project.source.0)?;
    let query = FileQuery {
        loaders: loaders.to_vec(),
        ..Default::default()
    };
    let mut screen = Screen::new()?;
    let result = request::wait(
        runtime,
        async {
            let selected = source.get_file(reference).await?;
            let page = source.list_files(&reference.project, &query).await?;
            Ok((selected, page))
        },
        "Fetching selected file and project files...",
        |status| {
            screen.terminal.draw(|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new(text(status)), frame.area());
            })?;
            Ok(())
        },
    )?;
    if let Some((selected, page)) = result {
        browse(
            source.as_ref(),
            runtime,
            query,
            vec![(
                reference.project.id.0.clone(),
                reference.project.clone(),
                page,
            )],
            Some(selected),
            screen,
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Files,
    Details,
}

struct Browser {
    table: TableState,
    pinned: bool,
    focus: Focus,
    full_details: bool,
    field: usize,
    detail_offset: usize,
    table_area: Rect,
    pinned_area: Rect,
    detail_area: Rect,
    visible_rows: usize,
    clipboard: Option<arboard::Clipboard>,
    status: Option<crate::widgets::Status>,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("table", &self.table)
            .field("pinned", &self.pinned)
            .field("focus", &self.focus)
            .field("full_details", &self.full_details)
            .field("field", &self.field)
            .field("detail_offset", &self.detail_offset)
            .field("table_area", &self.table_area)
            .field("pinned_area", &self.pinned_area)
            .field("detail_area", &self.detail_area)
            .field("visible_rows", &self.visible_rows)
            .field("status", &self.status)
            .field("clipboard_available", &self.clipboard.is_some())
            .finish()
    }
}

impl Browser {
    fn new(pinned: bool, count: usize) -> Self {
        Self {
            table: TableState::default().with_selected((!pinned && count > 0).then_some(0)),
            pinned,
            focus: Focus::Files,
            full_details: false,
            field: 0,
            detail_offset: 0,
            table_area: Rect::default(),
            pinned_area: Rect::default(),
            detail_area: Rect::default(),
            visible_rows: 1,
            clipboard: None,
            status: None,
        }
    }

    fn file<'a>(
        &self,
        page: &'a FilePage,
        pinned: Option<&'a ProjectFile>,
    ) -> Option<&'a ProjectFile> {
        if self.pinned {
            pinned
        } else {
            page.files.get(self.table.selected()?)
        }
    }

    fn select(&mut self, row: usize, count: usize) {
        self.pinned = false;
        self.table
            .select((count > 0).then_some(row.min(count.saturating_sub(1))));
        self.detail_offset = 0;
    }

    fn select_pinned(&mut self) {
        self.pinned = true;
        self.table.select(None);
        self.detail_offset = 0;
    }

    fn move_by(&mut self, amount: isize, count: usize, has_pinned: bool) {
        if self.focus == Focus::Details {
            self.field = self
                .field
                .saturating_add_signed(amount)
                .min(view::FIELD_COUNT - 1);
        } else if self.pinned {
            if amount > 0 && count > 0 {
                self.select(amount as usize - 1, count);
            }
        } else {
            let row = self.table.selected().unwrap_or(0);
            if has_pinned && amount < 0 && row < amount.unsigned_abs() {
                self.select_pinned();
            } else {
                self.select(row.saturating_add_signed(amount), count);
            }
        }
    }

    fn copy_field(&mut self, page: &FilePage, pinned: Option<&ProjectFile>) {
        if let Some(file) = self.file(page, pinned) {
            let (label, value) = view::field_value(file, self.field);
            self.status = Some(browser::copy_field(&mut self.clipboard, value, label));
        }
    }
}

fn browse(
    source: &dyn Source,
    runtime: &Runtime,
    mut query: FileQuery,
    mut projects: Vec<(String, ProjectRef, FilePage)>,
    selected_file: Option<ProjectFile>,
    mut screen: Screen,
) -> Result<()> {
    let mut current = 0;
    let mut browser = Browser::new(selected_file.is_some(), projects[0].2.files.len());
    let mut held = None;
    loop {
        let (label, reference, page) = &projects[current];
        screen.terminal.draw(|frame| {
            view::draw(
                frame,
                label,
                reference,
                page,
                selected_file.as_ref(),
                &query,
                current,
                projects.len(),
                &mut browser,
            )
        })?;
        if held.is_some() && !event::poll(Duration::from_millis(180))? {
            held = None;
        }
        let next = event::read()?;
        if let Event::Key(key) = &next {
            if held == Some(key.code) {
                if key.kind == KeyEventKind::Release {
                    held = None;
                }
                continue;
            }
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            held = None;
        }
        let action = input::handle(
            &mut browser,
            next.clone(),
            page,
            selected_file.as_ref(),
            query.limit,
        );
        let offset = match action {
            input::Action::Quit => return Ok(()),
            input::Action::Interrupt => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "file browsing interrupted",
                )
                .into());
            }
            input::Action::Project(backwards) => {
                current = if backwards {
                    (current + projects.len() - 1) % projects.len()
                } else {
                    (current + 1) % projects.len()
                };
                browser = Browser::new(selected_file.is_some(), projects[current].2.files.len());
                continue;
            }
            input::Action::Page(offset) => offset,
            input::Action::Redraw => continue,
        };
        if let Event::Key(key) = next {
            held = Some(key.code);
        }
        let mut pending = query.clone();
        pending.offset = offset;
        let backwards = offset < page.offset;
        match request::wait(
            runtime,
            source.list_files(reference, &pending),
            "Fetching files...",
            |loading| {
                browser.status = Some((loading.to_owned(), Role::Muted).into());
                screen.terminal.draw(|frame| {
                    view::draw(
                        frame,
                        label,
                        reference,
                        page,
                        selected_file.as_ref(),
                        &query,
                        current,
                        projects.len(),
                        &mut browser,
                    )
                })?;
                Ok(())
            },
        ) {
            Ok(Some(next)) if !next.files.is_empty() => {
                let count = next.files.len();
                browser.table = TableState::default();
                if count == 0 && selected_file.is_some() {
                    browser.select_pinned();
                } else {
                    browser.select(
                        if backwards {
                            count.saturating_sub(1)
                        } else {
                            0
                        },
                        count,
                    );
                }
                projects[current].2 = next;
                query = pending;
                browser.status = None;
            }
            Ok(Some(_)) => browser.status = Some(("No more files.".into(), Role::Muted).into()),
            Ok(None) => browser.status = Some(("Cancelled.".into(), Role::Muted).into()),
            Err(error) if request::is_interrupted(&error) => return Err(error),
            Err(error) => browser.status = Some(crate::widgets::Status::error(error)),
        }
    }
}
