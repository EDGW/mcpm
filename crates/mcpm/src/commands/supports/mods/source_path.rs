//! Shared archive path display options for mod commands.

use std::path::Path;

use crate::theme::{self, Role};
use clap::{Args, ValueEnum};
use owo_colors::Style;

#[derive(Args, Debug)]
pub(in crate::commands) struct PathArgs {
    /// Show the innermost filename, full archive chain, or outer filename.
    #[arg(long, value_enum, default_value = "full", value_name = "STYLE")]
    path_style: PathStyle,
}

impl PathArgs {
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

#[derive(Clone, Copy, ValueEnum, Debug)]
enum PathStyle {
    Inner,
    Full,
    Outer,
}
