//! Render the completion popup with ratatui list selection and shared browser styling.

use ratatui::{
    Frame,
    layout::Rect,
    text::Span,
    widgets::{Clear, List, ListItem, Paragraph},
};

use super::{Completion, CompletionField, Versions};
use crate::{
    display::style::text,
    theme::{
        Role,
        widgets::{self, style},
    },
    widgets::browser::{panel, selection, short_line},
};

impl Completion {
    pub fn draw(&mut self, frame: &mut Frame, search: Rect) {
        self.area = Rect::default();
        let Some(context) = &self.context else {
            return;
        };
        let available = frame.area().bottom().saturating_sub(search.bottom());
        if available < 3 {
            return;
        }
        let title = match context.field {
            CompletionField::Loader => " Loaders · ↑↓ · Enter/Tab · Esc ",
            CompletionField::Version => " Versions · ↑↓ · Enter/Tab · Esc ",
        };
        let message = if self.candidates.is_empty() {
            match (&context.field, &self.versions) {
                (CompletionField::Version, Versions::Loading | Versions::Unloaded) => {
                    vec!["Loading versions…".into()]
                }
                (CompletionField::Version, Versions::Failed(error)) => {
                    let mut lines = vec!["Ctrl-R retry · type manually".into()];
                    lines.extend(error.lines());
                    lines
                }
                _ => vec!["No matches · type manually".into()],
            }
        } else {
            Vec::new()
        };
        let width = search.width.saturating_sub(2).min(64);
        let height = (if self.candidates.is_empty() {
            message.len().max(1)
        } else {
            self.candidates.len().clamp(1, 6)
        } as u16
            + 2)
        .min(available);
        self.area = Rect::new(search.x + 1, search.bottom(), width, height);
        frame.render_widget(Clear, self.area);
        let block = panel(title, true)
            .border_style(style(Role::Secondary))
            .title_style(style(Role::Secondary));
        if self.candidates.is_empty() {
            let paint = match context.field {
                CompletionField::Version => Role::Normal,
                CompletionField::Loader => Role::Muted,
            };
            frame.render_widget(
                Paragraph::new(
                    message
                        .into_iter()
                        .map(|line| {
                            short_line(
                                vec![Span::styled(line, style(paint))],
                                width.saturating_sub(2) as usize,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .block(block),
                self.area,
            );
        } else {
            let items = self.candidates.iter().map(|value| {
                let paint = if context.field == CompletionField::Loader {
                    widgets::loader(value)
                } else {
                    style(Role::Normal)
                };
                ListItem::new(short_line(
                    vec![Span::styled(text(value), paint)],
                    width.saturating_sub(2) as usize,
                ))
            });
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_style(selection(true)),
                self.area,
                &mut self.state,
            );
        }
    }
}
