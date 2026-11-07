//! Editable command recall for one frontend attachment; never loads or sends server history.

#[derive(Default)]
pub(super) struct CommandHistory {
    entries: Vec<String>,
    selected: Option<usize>,
}

impl CommandHistory {
    /// Preserve the exact submitted text, including builtin escapes and leading spaces.
    pub fn record(&mut self, text: &str) {
        self.reset();
        if !text.is_empty() {
            self.entries.push(text.to_owned());
        }
    }

    /// Start only from an empty draft; a recalled draft stays navigable until edited.
    /// Returns false when arrows should instead navigate command suggestions.
    pub fn navigate(&mut self, draft: &mut String, down: bool) -> bool {
        if self.selected.is_none() && !draft.is_empty() {
            return false;
        }
        self.selected = match (self.selected, down) {
            (None, false) => self.entries.len().checked_sub(1),
            (Some(index), false) => Some(index.saturating_sub(1)),
            (Some(index), true) if index + 1 < self.entries.len() => Some(index + 1),
            _ => None,
        };
        draft.clear();
        if let Some(index) = self.selected {
            draft.push_str(&self.entries[index]);
        }
        true
    }

    /// Editing the recalled copy never changes a saved entry.
    pub fn reset(&mut self) {
        self.selected = None;
    }
}
