//! Shared archive path display options for mod commands.

use std::path::Path;

use clap::{Args, ValueEnum};
use owo_colors::Style;

use crate::commands::output_style::{OutputStyle, text};

#[derive(Args)]
pub(in crate::commands) struct PathArgs {
    /// Show the innermost filename, full archive chain, or outer filename.
    #[arg(long, value_enum, default_value = "full", value_name = "STYLE")]
    path_style: PathStyle,
}

impl PathArgs {
    /// Shared styled pieces keep list column measurement and inspection output consistent.
    pub(in crate::commands) fn pieces(
        &self,
        outer: &Path,
        nested: &[String],
    ) -> Vec<(String, Style)> {
        let mut pieces = Vec::new();
        for (index, part) in self.parts(outer, nested).into_iter().enumerate() {
            if index > 0 {
                pieces.push(("!".to_owned(), Style::new().bright_white().bold()));
            }
            pieces.push((part, Style::new().bright_black()));
        }
        pieces
    }

    pub(in crate::commands) fn format(
        &self,
        outer: &Path,
        nested: &[String],
        style: &OutputStyle,
    ) -> String {
        self.pieces(outer, nested)
            .into_iter()
            .map(|(part, paint)| style.paint(text(&part), paint))
            .collect()
    }

    /// Separate archive levels so renderers can highlight only the separators.
    fn parts(&self, outer: &Path, nested: &[String]) -> Vec<String> {
        let outer_filename = || {
            outer
                .file_name()
                .unwrap_or(outer.as_os_str())
                .to_string_lossy()
                .into_owned()
        };
        match self.path_style {
            PathStyle::Inner => vec![
                nested
                    .last()
                    .map(|member| member.rsplit('/').next().unwrap_or(member).to_owned())
                    .unwrap_or_else(outer_filename),
            ],
            PathStyle::Full => std::iter::once(outer.display().to_string())
                .chain(nested.iter().cloned())
                .collect(),
            PathStyle::Outer => vec![outer_filename()],
        }
    }
}

/// Full outer path followed by every nested archive, with white archive separators.
pub(in crate::commands) fn full(outer: &Path, nested: &[String], style: &OutputStyle) -> String {
    PathArgs {
        path_style: PathStyle::Full,
    }
    .format(outer, nested, style)
}

#[derive(Clone, Copy, ValueEnum)]
enum PathStyle {
    Inner,
    Full,
    Outer,
}
