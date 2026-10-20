//! Column selection for remote project search lists.

use clap::{Args, ValueEnum};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum Column {
    Index,
    Source,
    Name,
    Slug,
    Id,
    Summary,
    Url,
}

impl Column {
    const ALL: [Self; 7] = [
        Self::Index,
        Self::Source,
        Self::Name,
        Self::Slug,
        Self::Id,
        Self::Summary,
        Self::Url,
    ];

    fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Source => 'O',
            Self::Name => 'N',
            Self::Slug => 'S',
            Self::Id => 'P',
            Self::Summary => 'D',
            Self::Url => 'U',
        }
    }
}

#[derive(Args)]
pub(super) struct ColumnArgs {
    /// List columns: I=index, O=source, N=name, S=slug, P=project ID, D=summary, U=url. Disables TUI.
    #[arg(short = 'C', value_name = "IONSPDU", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Comma-separated list columns; combined with -C. Disables TUI.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "COLUMNS")]
    columns: Vec<Column>,
}

impl ColumnArgs {
    pub(super) fn is_explicit(&self) -> bool {
        !self.compact.is_empty() || !self.columns.is_empty()
    }

    pub(super) fn selected(&self) -> Vec<Column> {
        Column::ALL
            .into_iter()
            .filter(|column| {
                if !self.is_explicit() {
                    *column != Column::Source
                } else {
                    self.columns.contains(column)
                        || self.compact.iter().any(|s| s.contains(column.letter()))
                }
            })
            .collect()
    }
}

fn parse_letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|ch| "IONSPDU".contains(ch)) {
        return Err("expected column letters I, O, N, S, P, D, U (for example INP)".into());
    }
    Ok(value.to_owned())
}
