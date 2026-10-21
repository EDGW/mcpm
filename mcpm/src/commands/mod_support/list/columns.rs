//! Column selection for shared mod list rendering.

use clap::{Args, ValueEnum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(super) enum Column {
    Index,
    Name,
    Modid,
    Filename,
    Loaders,
}

impl Column {
    pub(super) const ALL: [Self; 5] = [
        Self::Index,
        Self::Name,
        Self::Modid,
        Self::Filename,
        Self::Loaders,
    ];

    pub(super) fn position(self) -> usize {
        self as usize
    }

    fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Name => 'N',
            Self::Modid => 'M',
            Self::Filename => 'F',
            Self::Loaders => 'L',
        }
    }
}

#[derive(Args)]
pub(super) struct ColumnArgs {
    /// Select columns by letters: I=index, N=name, M=modid, F=filename, L=loaders.
    #[arg(short = 'C', value_name = "INMFL", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Select comma-separated columns, combined with -C and --column.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "COLUMNS")]
    columns: Vec<Column>,
    /// Select one column; repeat to select multiple columns.
    #[arg(long, value_enum, value_name = "COLUMN")]
    column: Vec<Column>,
}

impl ColumnArgs {
    pub(super) fn selected(&self) -> Vec<Column> {
        let default = self.compact.is_empty() && self.columns.is_empty() && self.column.is_empty();
        Column::ALL
            .into_iter()
            .filter(|column| {
                default
                    || self.columns.contains(column)
                    || self.column.contains(column)
                    || self
                        .compact
                        .iter()
                        .any(|letters| letters.contains(column.letter()))
            })
            .collect()
    }
}

fn parse_letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|letter| "INMFL".contains(letter)) {
        return Err("expected column letters I, N, M, F, L (for example INMFL)".into());
    }
    Ok(value.to_owned())
}
