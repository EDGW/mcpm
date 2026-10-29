//! Search value suggestions, shared session version catalog and draft-only completion state.

use clap::ValueEnum;
use ratatui::{layout::Rect, widgets::ListState};

use super::super::expression::{CompletionContext, CompletionField, completion_context};
use crate::commands::supports::mods::loaders::Loader;

mod view;

#[cfg(test)]
mod tests;

/// Search completion state and its suggestion view.
///
/// # Implementations
/// - [`Self::refresh`]: candidate selection and completion state.
/// - [`Self::draw`]: suggestion rendering in `view`.
#[derive(Default, Debug)]
pub(super) struct Completion {
    context: Option<CompletionContext>,
    candidates: Vec<String>,
    state: ListState,
    versions: Versions,
    dismissed: Option<(String, usize)>,
    pub area: Rect,
}

impl Completion {
    pub fn refresh(&mut self, draft: &str, cursor: usize, focused: bool) {
        let context = if focused
            && self
                .dismissed
                .as_ref()
                .is_none_or(|(text, at)| text != draft || *at != cursor)
        {
            completion_context(draft, cursor)
        } else {
            None
        };
        if context != self.context {
            self.state = ListState::default();
            self.area = Rect::default();
        }
        self.candidates.clear();
        if let Some(context) = &context {
            let values = match context.field {
                CompletionField::Loader => Loader::value_variants()
                    .iter()
                    .map(|loader| loader.id().to_owned())
                    .collect::<Vec<_>>(),
                CompletionField::Version => match &self.versions {
                    Versions::Ready(values) => values.clone(),
                    _ => Vec::new(),
                },
            };
            let prefix = context.prefix.to_ascii_lowercase();
            self.candidates = values
                .into_iter()
                .filter(|value| value.to_ascii_lowercase().starts_with(&prefix))
                .collect();
        }
        self.context = context;
        self.state.select(
            (!self.candidates.is_empty()).then_some(
                self.state
                    .selected()
                    .unwrap_or(0)
                    .min(self.candidates.len().saturating_sub(1)),
            ),
        );
    }

    pub fn open(&self) -> bool {
        self.context.is_some()
    }

    pub fn needs_versions(&self) -> bool {
        matches!(self.versions, Versions::Unloaded)
    }

    pub fn version_catalog(&self) -> Option<&[String]> {
        match &self.versions {
            Versions::Ready(values) => Some(values),
            _ => None,
        }
    }

    pub fn loading(&mut self) {
        self.versions = Versions::Loading;
    }

    pub fn finish_versions(&mut self, result: anyhow::Result<Vec<String>>) {
        self.versions = match result {
            Ok(values) => Versions::Ready(values),
            Err(error) => Versions::Failed(crate::widgets::Status::error(error)),
        };
    }

    pub fn retry_versions(&mut self) -> bool {
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.field == CompletionField::Version)
            && matches!(self.versions, Versions::Failed(_))
        {
            self.versions = Versions::Unloaded;
            true
        } else {
            false
        }
    }

    pub fn move_by(&mut self, amount: isize) {
        if let Some(selected) = self.state.selected() {
            self.state.select(Some(
                selected
                    .saturating_add_signed(amount)
                    .min(self.candidates.len() - 1),
            ));
        }
    }

    pub fn hide(&mut self, draft: &str, cursor: usize) {
        self.dismissed = Some((draft.into(), cursor));
        self.context = None;
        self.candidates.clear();
        self.area = Rect::default();
    }

    pub fn accept(&mut self, draft: &mut String, cursor: &mut usize) -> bool {
        let Some(context) = &self.context else {
            return false;
        };
        let Some(value) = self
            .state
            .selected()
            .and_then(|index| self.candidates.get(index))
        else {
            return false;
        };
        let replacement = format!("{}{value}", if context.separator { " " } else { "" });
        draft.replace_range(context.range.clone(), &replacement);
        *cursor = context.range.start + replacement.len();
        self.hide(draft, *cursor);
        true
    }

    pub fn click(&mut self, row: u16) -> bool {
        let index = self.state.offset() + usize::from(row.saturating_sub(self.area.y + 1));
        if row > self.area.y
            && row < self.area.bottom().saturating_sub(1)
            && index < self.candidates.len()
        {
            self.state.select(Some(index));
            true
        } else {
            false
        }
    }
}

#[derive(Default, Debug)]
enum Versions {
    #[default]
    Unloaded,
    Loading,
    Ready(Vec<String>),
    Failed(crate::widgets::Status),
}
