//! Shared styling helpers for server command help.

use clap::builder::StyledStr;
use owo_colors::OwoColorize;

/// Line-oriented builder that keeps styling out of each command's text flow.
pub(super) struct Help(String);

impl Help {
    pub(super) fn new() -> Self {
        Self(String::new())
    }

    pub(super) fn heading(&mut self, text: &str) {
        self.line(&format!("{}", text.bold().underline()));
    }

    pub(super) fn line(&mut self, text: &str) {
        self.0.push_str(text);
        self.0.push('\n');
    }

    pub(super) fn blank(&mut self) {
        self.0.push('\n');
    }

    pub(super) fn finish(mut self) -> StyledStr {
        while self.0.ends_with('\n') {
            self.0.pop();
        }
        StyledStr::from(self.0)
    }
}

pub(super) fn bold(text: &str) -> String {
    text.bold().to_string()
}
