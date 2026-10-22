//! Shared archive path display options for mod commands.

use std::path::Path;

use crate::theme::{self, Role};
use clap::{Args, ValueEnum};
use owo_colors::Style;

use crate::commands::output_style::{OutputStyle, text};

#[derive(Args, Debug)]
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
        let filename = outer
            .file_name()
            .unwrap_or(outer.as_os_str())
            .to_string_lossy();
        self.pieces_with_filename(outer, nested, &filename)
    }

    pub(super) fn pieces_with_filename(
        &self,
        outer: &Path,
        nested: &[String],
        filename: &str,
    ) -> Vec<(String, Style)> {
        let mut pieces = Vec::new();
        for (index, part) in self
            .parts_with_filename(outer, nested, filename)
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                pieces.push(("!".to_owned(), theme::console(Role::Important)));
            }
            pieces.push((part, theme::console(Role::Muted)));
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

    /// A remote filename is already parsed on its owning platform.
    pub(super) fn parts_with_filename(
        &self,
        outer: &Path,
        nested: &[String],
        filename: &str,
    ) -> Vec<String> {
        match self.path_style {
            PathStyle::Inner => vec![
                nested
                    .last()
                    .map(|member| member.rsplit('/').next().unwrap_or(member).to_owned())
                    .unwrap_or_else(|| filename.to_owned()),
            ],
            PathStyle::Full => std::iter::once(outer.display().to_string())
                .chain(nested.iter().cloned())
                .collect(),
            PathStyle::Outer => vec![filename.to_owned()],
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

#[derive(Clone, Copy, ValueEnum, Debug)]
enum PathStyle {
    Inner,
    Full,
    Outer,
}
