use std::path::Path;

use clap::{Args, ValueEnum};

#[derive(Args)]
pub(super) struct PathArgs {
    /// Show the innermost filename, full archive chain, or outer filename.
    #[arg(long, value_enum, default_value = "full", value_name = "STYLE")]
    path_style: PathStyle,
}

#[derive(Clone, Copy, ValueEnum)]
enum PathStyle {
    Inner,
    Full,
    Outer,
}

impl PathArgs {
    /// Separate archive levels so renderers can highlight only the separators.
    pub(super) fn parts(&self, outer: &Path, nested: &[String]) -> Vec<String> {
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
            PathStyle::Full => std::iter::once(if nested.is_empty() {
                outer.display().to_string()
            } else {
                outer_filename()
            })
            .chain(nested.iter().cloned())
            .collect(),
            PathStyle::Outer => vec![outer_filename()],
        }
    }
}
