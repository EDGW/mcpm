//! Shared list/detail layout, table sizing, focus styling and terminal ownership.

use std::io;

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Cell, List, ListItem, ListState, Padding, Row, Table},
};

use super::{MatchedText, Overflow};
use crate::theme::{
    Role,
    widgets::{self, style},
};

pub(crate) struct Screen {
    pub terminal: Terminal<CrosstermBackend<io::Stderr>>,
    raw: bool,
    alternate: bool,
}

impl Screen {
    pub fn new() -> io::Result<Self> {
        let mut screen = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stderr()))?,
            raw: false,
            alternate: false,
        };
        terminal::enable_raw_mode()?;
        screen.raw = true;
        screen.alternate = true;
        execute!(
            screen.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide
        )?;
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(
                self.terminal.backend_mut(),
                DisableMouseCapture,
                Show,
                LeaveAlternateScreen
            );
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

pub(crate) struct BrowserLayout {
    pub title: Rect,
    pub pinned: Rect,
    pub table: Rect,
    pub accessory: Rect,
    pub details: Rect,
    pub footer: Rect,
}

impl BrowserLayout {
    pub fn new(
        area: Rect,
        full_details: bool,
        details_focused: bool,
        accessory: u16,
        pinned: u16,
        detail_height: u16,
    ) -> Self {
        let [title, content, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .areas(area);
        if full_details {
            return Self {
                title,
                details: content,
                footer,
                pinned: Rect::default(),
                table: Rect::default(),
                accessory: Rect::default(),
            };
        }
        let detail_height =
            if content.height.saturating_sub(pinned) >= detail_height.saturating_add(6) {
                detail_height
            } else if details_focused {
                content.height.min(detail_height)
            } else {
                0
            };
        let [pinned, table, accessory, details] = Layout::vertical([
            Constraint::Length(pinned),
            Constraint::Min(0),
            Constraint::Length(accessory),
            Constraint::Length(detail_height),
        ])
        .areas(content);
        Self {
            title,
            pinned,
            table,
            accessory,
            details,
            footer,
        }
    }
}

pub(crate) fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::bordered()
        .title(title)
        .border_style(style(if focused { Role::Primary } else { Role::Muted }))
}

pub(crate) fn selection(focused: bool) -> Style {
    if focused {
        widgets::selection()
    } else {
        Style::default()
    }
}

pub(crate) fn short_line(spans: Vec<Span<'_>>, width: usize) -> Line<'static> {
    MatchedText::new(Line::from(spans))
        .overflow(Overflow::Ellipsis)
        .ellipsis_style(style(Role::Muted))
        .line(width)
}

/// Exact allocations shared by clipping and rendering, including optional tree markers.
pub(crate) struct TableColumns {
    pub areas: Vec<Rect>,
    pub marker_width: u16,
    marker: bool,
    pub spacing: u16,
}

impl TableColumns {
    pub fn new(width: u16, weights: &[u16], marker: u16) -> Self {
        let marker_width = width.min(marker);
        let spacing = u16::from(width > weights.len() as u16 * 3);
        let gaps = weights.len().saturating_sub(usize::from(marker == 0)) as u16;
        let available = width
            .saturating_sub(marker_width)
            .saturating_sub(spacing * gaps);
        let areas = Layout::horizontal(weights.iter().map(|&weight| Constraint::Fill(weight)))
            .split(Rect::new(0, 0, available, 1))
            .to_vec();
        Self {
            areas,
            marker_width,
            spacing,
            marker: marker > 0,
        }
    }

    pub fn table<'a>(
        &self,
        rows: impl IntoIterator<Item = Row<'a>>,
        header: Row<'a>,
        block: Block<'a>,
        focused: bool,
    ) -> Table<'a> {
        let mut widths = Vec::new();
        if self.marker {
            widths.push(Constraint::Length(self.marker_width));
        }
        widths.extend(self.areas.iter().map(|area| Constraint::Length(area.width)));
        Table::new(rows, widths)
            .header(header)
            .block(block)
            .column_spacing(self.spacing)
            .row_highlight_style(selection(focused))
    }

    pub fn header(&self, labels: &[&str]) -> Row<'static> {
        let mut cells = Vec::new();
        if self.marker {
            cells.push(Cell::from(""));
        }
        cells.extend(labels.iter().zip(&self.areas).map(|(label, area)| {
            Cell::from(short_line(
                vec![Span::raw(label.to_string())],
                area.width as usize,
            ))
        }));
        Row::new(cells).style(style(Role::Muted)).bottom_margin(1)
    }
}

pub(crate) fn draw_fields(
    frame: &mut Frame,
    area: Rect,
    block: Block<'_>,
    items: Vec<ListItem<'_>>,
    field: usize,
    offset: &mut usize,
    focused: bool,
) {
    let mut state = ListState::default()
        .with_offset(*offset)
        .with_selected(Some(field));
    frame.render_stateful_widget(
        List::new(items)
            .block(block.padding(Padding::horizontal(1)))
            .highlight_style(selection(focused)),
        area,
        &mut state,
    );
    *offset = state.offset();
}

pub(crate) fn copy_field(
    clipboard: &mut Option<arboard::Clipboard>,
    value: String,
    label: &str,
) -> (String, Role) {
    let result = (|| {
        if clipboard.is_none() {
            *clipboard = Some(arboard::Clipboard::new()?);
        }
        clipboard.as_mut().unwrap().set_text(value)
    })();
    match result {
        Ok(()) => (format!("Copied {label}"), Role::Success),
        Err(error) => (format!("Copy failed: {error}"), Role::Danger),
    }
}
