//! Interactive mod browsing, filtering and navigation over shared semantic rows.

use super::{
    ListedFile,
    columns::Column,
    filter::{self, Filter},
    rows::RowOptions,
};
use crate::commands::{execution::Runtime, source_support::identify::Location};
use anyhow::{Context, Result};
use crossterm::event;
use kako_mods_lib::source::FileRef;
use ratatui::widgets::TableState;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

mod identify;
mod input;
mod screen;
mod tree;
mod view;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) struct Summary<'a> {
    pub location: Location<'a>,
    pub ignored: usize,
    pub ignored_files: &'a [PathBuf],
    pub input_errors: usize,
    pub failed: usize,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(
    files: &[ListedFile],
    columns: &[Column],
    options: RowOptions<'_>,
    source_loaders: &[String],
    identify_parallelism: usize,
    filter: Filter,
    summary: Summary<'_>,
) -> Result<()> {
    let _console = crate::logging::pause_console();
    let runtime = Runtime::new()?;
    let _guard = runtime.enter();
    let mut browser = Browser::new(tree::build(files, &options), filter);
    tracing::debug!(
        nodes = browser.nodes.len(),
        visible = browser.matches.len(),
        local_files = summary.location.is_local(),
        identify_parallelism,
        "Initialized mod list browser"
    );
    let updates = identify::start(
        files
            .iter()
            .map(|file| file.scanned.file.entry.path().to_owned())
            .collect(),
        identify_parallelism,
        summary.location,
    )?;
    let mut screen = screen::Screen::new().context("failed to open mod list TUI")?;
    let mut redraw = true;
    let mut page = 1;
    let started = Instant::now();
    loop {
        let reference = runtime.block_on(async {
            loop {
                for update in updates.try_iter() {
                    browser.apply_identification(update);
                    redraw = true;
                }
                let tick = (started.elapsed().as_millis() / 100) as u64;
                if tick != browser.tick {
                    browser.tick = tick;
                    redraw |= browser
                        .identification
                        .iter()
                        .flatten()
                        .any(|state| matches!(state, identify::State::Loading));
                }
                if redraw {
                    screen
                        .terminal
                        .draw(|frame| {
                            page = view::draw(frame, &mut browser, columns, &summary);
                        })
                        .context("failed to draw mod list TUI")?;
                    redraw = false;
                }
                let batch_started = Instant::now();
                while event::poll(Duration::ZERO).context("failed to poll mod list input")? {
                    let next = event::read().context("failed to read mod list input")?;
                    match input::handle(&mut browser, &summary, next, page) {
                        input::Update::Quit => return Ok::<_, anyhow::Error>(None),
                        input::Update::OpenFiles(reference) => return Ok(Some(reference)),
                        input::Update::Redraw => redraw = true,
                        input::Update::Ignore => {}
                    }
                    if batch_started.elapsed() >= Duration::from_millis(8) {
                        break;
                    }
                }
                // Yield to source lookups without blocking input or animation.
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })?;
        let Some(reference) = reference else {
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis(),
                "Closed mod list TUI"
            );
            return Ok(());
        };
        // Transfer terminal ownership outside block_on; the file browser drives
        // the same runtime while the mod tree and lookup results remain alive.
        tracing::debug!(?reference, "Opening source files from mod list");
        drop(screen);
        let result = crate::commands::source_support::files::open_selected(
            &runtime,
            &reference,
            source_loaders,
        );
        if result.as_ref().err().is_some_and(|error| {
            error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::Interrupted)
        }) {
            return result;
        }
        screen = screen::Screen::new().context("failed to restore mod list TUI")?;
        browser.status = result.err().map(|error| {
            tracing::debug!(error = %format_args!("{error:#}"), "Could not open source files");
            (
                format!("Could not open files: {error:#}"),
                crate::theme::Role::Danger,
            )
        });
        redraw = true;
    }
}

struct Browser {
    nodes: Vec<tree::Node>,
    identification: Vec<[identify::State; 2]>,
    tick: u64,
    detail_offset: usize,
    focus: Focus,
    field: usize,
    status: Option<(String, crate::theme::Role)>,
    clipboard: Option<arboard::Clipboard>,
    matches: Vec<usize>,
    table: TableState,
    query: String,
    parsed: Filter,
    search_overrides: HashMap<usize, bool>,
    search_included: Vec<bool>,
    filter_open: bool,
    query_cursor: usize,
    filter_area: ratatui::layout::Rect,
    details: bool,
    ignored: bool,
    scroll: u16,
    scroll_limit: u16,
    table_area: ratatui::layout::Rect,
    detail_area: ratatui::layout::Rect,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("nodes", &self.nodes)
            .field("identification", &self.identification)
            .field("tick", &self.tick)
            .field("detail_offset", &self.detail_offset)
            .field("focus", &self.focus)
            .field("field", &self.field)
            .field("status", &self.status)
            .field("matches", &self.matches)
            .field("table", &self.table)
            .field("query", &self.query)
            .field("parsed", &self.parsed)
            .field("search_overrides", &self.search_overrides)
            .field("search_included", &self.search_included)
            .field("filter_open", &self.filter_open)
            .field("query_cursor", &self.query_cursor)
            .field("filter_area", &self.filter_area)
            .field("details", &self.details)
            .field("ignored", &self.ignored)
            .field("scroll", &self.scroll)
            .field("scroll_limit", &self.scroll_limit)
            .field("table_area", &self.table_area)
            .field("detail_area", &self.detail_area)
            .field("clipboard_available", &self.clipboard.is_some())
            .finish()
    }
}
impl Browser {
    fn new(nodes: Vec<tree::Node>, filter: Filter) -> Self {
        let query = filter.as_str().to_owned();
        let identification = vec![
            [identify::State::Queued, identify::State::Queued];
            nodes.iter().map(|node| node.file + 1).max().unwrap_or(0)
        ];
        let mut browser = Self {
            nodes,
            identification,
            tick: 0,
            detail_offset: 0,
            focus: Focus::Mods,
            field: 0,
            status: None,
            clipboard: None,
            matches: Vec::new(),
            table: TableState::default(),
            query_cursor: query.len(),
            filter_open: !query.trim().is_empty(),
            query,
            parsed: filter,
            search_overrides: HashMap::new(),
            search_included: Vec::new(),
            filter_area: ratatui::layout::Rect::default(),
            details: false,
            ignored: false,
            scroll: 0,
            scroll_limit: 0,
            table_area: ratatui::layout::Rect::default(),
            detail_area: ratatui::layout::Rect::default(),
        };
        browser.filter();
        browser
    }
    fn apply_identification(&mut self, update: identify::Update) {
        for file in update.files {
            self.identification[file][update.platform.index()] = update.state.clone();
        }
    }
    fn identification_progress(&self) -> Option<(usize, usize)> {
        let total = self.identification.len() * identify::Platform::ALL.len();
        let completed = self
            .identification
            .iter()
            .flatten()
            .filter(|state| !matches!(state, identify::State::Queued | identify::State::Loading))
            .count();
        (completed < total).then_some((completed, total))
    }
    fn spinner(&self) -> String {
        indicatif::ProgressStyle::default_spinner()
            .get_tick_str(self.tick)
            .to_owned()
    }
    fn detail_value(&self, field: DetailField) -> Option<String> {
        let node = &self.nodes[self.selected_index()?];
        Some(match field {
            DetailField::Column(column) => node.row.cells[column.position()]
                .iter()
                .map(|piece| piece.text.as_str())
                .collect(),
            DetailField::Source(platform) if node.outer => {
                self.identification[node.file][platform.index()].details(&self.spinner())
            }
            DetailField::Source(_) => "Not checked (nested JAR)".into(),
        })
    }
    fn selected_source_file(&self) -> Option<&FileRef> {
        if self.focus != Focus::Details || self.ignored {
            return None;
        }
        let DetailField::Source(platform) = DETAIL_FIELDS[self.field] else {
            return None;
        };
        let node = &self.nodes[self.selected_index()?];
        if !node.outer {
            return None;
        }
        match &self.identification[node.file][platform.index()] {
            identify::State::Matched(found) => Some(&found.reference),
            _ => None,
        }
    }
    fn filter(&mut self) {
        let previous = self
            .table
            .selected()
            .and_then(|index| self.matches.get(index))
            .copied();
        if self.parsed.as_str() != self.query {
            self.parsed = Filter::parse(&self.query);
        }
        let query = self.query.trim();
        let parsed = &self.parsed;
        let mut included = vec![false; self.nodes.len()];
        if !query.is_empty() {
            for (index, node) in self.nodes.iter().enumerate() {
                if parsed.matches(&node.row) {
                    included[index] = true;
                    let mut parent = node.parent;
                    while let Some(index) = parent {
                        included[index] = true;
                        parent = self.nodes[index].parent;
                    }
                }
            }
        }
        // Search is a temporary projection: only ancestors of matching rows
        // need opening, and overrides expire when their branch leaves the results.
        self.search_overrides
            .retain(|&index, _| !query.is_empty() && included[index]);
        self.search_included = included;
        let expanded: Vec<_> = (0..self.nodes.len())
            .map(|index| self.is_expanded(index))
            .collect();
        fn visit(
            nodes: &[tree::Node],
            index: usize,
            query: &str,
            included: &[bool],
            expanded: &[bool],
            visible: &mut Vec<usize>,
        ) {
            if !query.is_empty() && !included[index] {
                return;
            }
            visible.push(index);
            if expanded[index] {
                for &child in &nodes[index].children {
                    visit(nodes, child, query, included, expanded, visible);
                }
            }
        }
        self.matches.clear();
        for (index, node) in self.nodes.iter().enumerate() {
            if node.parent.is_none() {
                visit(
                    &self.nodes,
                    index,
                    query,
                    &self.search_included,
                    &expanded,
                    &mut self.matches,
                );
            }
        }
        self.table = TableState::default();
        let selected = previous
            .and_then(|previous| self.matches.iter().position(|&index| index == previous))
            .unwrap_or(0);
        self.select(selected);
        tracing::debug!(filter = %self.query, nodes = self.nodes.len(), visible = self.matches.len(), "Applied TUI mod filter");
    }
    fn open_filter(&mut self) {
        self.filter_open = true;
        self.focus = Focus::Filter;
        self.query_cursor = self.query.len();
        self.details = false;
        self.ignored = false;
    }
    fn close_filter(&mut self) {
        self.filter_open = false;
        self.focus = Focus::Mods;
    }
    fn cycle_focus(&mut self, backwards: bool) {
        self.focus = match (self.focus, backwards, self.filter_open) {
            (Focus::Mods, false, true) | (Focus::Details, true, true) => Focus::Filter,
            (Focus::Filter, false, _) | (Focus::Mods, true, _) | (Focus::Mods, false, false) => {
                Focus::Details
            }
            _ => Focus::Mods,
        };
        self.details = false;
        self.ignored = false;
    }
    fn selected_index(&self) -> Option<usize> {
        self.table
            .selected()
            .and_then(|index| self.matches.get(index))
            .copied()
    }
    fn is_expanded(&self, index: usize) -> bool {
        if self.query.trim().is_empty() {
            self.nodes[index].expanded
        } else {
            self.search_overrides
                .get(&index)
                .copied()
                .unwrap_or_else(|| {
                    self.nodes[index].expanded
                        || self.nodes[index]
                            .children
                            .iter()
                            .any(|&child| self.search_included[child])
                })
        }
    }
    fn set_expanded(&mut self, index: usize, open: bool) {
        if self.query.trim().is_empty() {
            self.nodes[index].expanded = open;
        } else {
            self.search_overrides.insert(index, open);
        }
    }
    fn toggle(&mut self) -> bool {
        let Some(index) = self.selected_index() else {
            return false;
        };
        if self.nodes[index].children.is_empty() {
            return false;
        }
        self.set_expanded(index, !self.is_expanded(index));
        self.filter();
        true
    }
    fn expand(&mut self, open: bool) -> bool {
        let Some(index) = self.selected_index() else {
            return false;
        };
        if !self.nodes[index].children.is_empty() && self.is_expanded(index) != open {
            self.set_expanded(index, open);
            self.filter();
            true
        } else if !open && let Some(parent) = self.nodes[index].parent {
            if let Some(position) = self.matches.iter().position(|&index| index == parent) {
                return self.select(position);
            }
            false
        } else {
            false
        }
    }
    fn expand_all(&mut self) {
        if self.query.trim().is_empty() {
            for node in &mut self.nodes {
                node.expanded = true;
            }
        } else {
            for (index, node) in self.nodes.iter().enumerate() {
                if self.search_included[index] && !node.children.is_empty() {
                    self.search_overrides.insert(index, true);
                }
            }
        }
        self.filter();
    }
    fn select(&mut self, index: usize) -> bool {
        let selected =
            (!self.matches.is_empty()).then_some(index.min(self.matches.len().saturating_sub(1)));
        if self.table.selected() == selected {
            return false;
        }
        self.table.select(selected);
        self.scroll = 0;
        true
    }
    fn scroll_by(&mut self, amount: isize) -> bool {
        let next = self
            .scroll
            .saturating_add_signed(amount.clamp(i16::MIN as isize, i16::MAX as isize) as i16)
            .min(self.scroll_limit);
        let changed = next != self.scroll;
        self.scroll = next;
        changed
    }
    fn move_by(&mut self, amount: isize) -> bool {
        if self.focus == Focus::Details && !self.ignored {
            let field = self
                .field
                .saturating_add_signed(amount)
                .min(DETAIL_FIELDS.len() - 1);
            let changed = field != self.field;
            self.field = field;
            changed
        } else if self.ignored {
            self.scroll_by(amount)
        } else {
            self.select(
                self.table
                    .selected()
                    .unwrap_or(0)
                    .saturating_add_signed(amount),
            )
        }
    }
    fn detail_field_at(&self, row: u16) -> Option<usize> {
        let mut remaining = usize::from(row);
        for (index, field) in DETAIL_FIELDS.iter().enumerate().skip(self.detail_offset) {
            if remaining < field.height() {
                return Some(index);
            }
            remaining -= field.height();
        }
        None
    }
    fn copy_field(&mut self) {
        let field = DETAIL_FIELDS[self.field];
        let Some(value) = self.detail_value(field) else {
            return;
        };
        self.status = Some(crate::widgets::browser::copy_field(
            &mut self.clipboard,
            value,
            field.label(),
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Mods,
    Filter,
    Details,
}
#[derive(Clone, Copy)]
enum DetailField {
    Column(Column),
    Source(identify::Platform),
}
impl DetailField {
    fn height(self) -> usize {
        match self {
            Self::Column(_) => 1,
            Self::Source(_) => 2,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Column(column) => view::label(column),
            Self::Source(platform) => platform.name(),
        }
    }
}
const DETAIL_FIELDS: [DetailField; 6] = [
    DetailField::Column(Column::Name),
    DetailField::Column(Column::Modid),
    DetailField::Column(Column::Loaders),
    DetailField::Column(Column::Filename),
    DetailField::Source(identify::Platform::Modrinth),
    DetailField::Source(identify::Platform::CurseForge),
];
