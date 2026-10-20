//! Column selection for list output.

use clap::{Args, ValueEnum};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum Column {
    Index,
    Name,
    State,
    Loaders,
    Detail,
}

impl Column {
    const ALL: [Self; 5] = [
        Self::Index,
        Self::Name,
        Self::State,
        Self::Loaders,
        Self::Detail,
    ];
    fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Name => 'N',
            Self::State => 'S',
            Self::Loaders => 'L',
            Self::Detail => 'D',
        }
    }
}

#[derive(Args)]
pub(super) struct ColumnArgs {
    /// Select list columns: I=index, N=name, S=state, L=loaders, D=detail.
    #[arg(short = 'C', value_name = "COLUMNS", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Comma-separated columns, combined with -C.
    #[arg(long, value_enum, value_delimiter = ',')]
    columns: Vec<Column>,
}

impl ColumnArgs {
    pub(super) fn is_explicit(&self) -> bool {
        !self.compact.is_empty() || !self.columns.is_empty()
    }
    pub(super) fn selected(&self) -> Vec<Column> {
        Column::ALL
            .into_iter()
            .filter(|c| {
                !self.is_explicit()
                    || self.columns.contains(c)
                    || self.compact.iter().any(|s| s.contains(c.letter()))
            })
            .collect()
    }
}

fn parse_letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|c| "INSLD".contains(c)) {
        return Err("expected column letters INSLD".into());
    }
    Ok(value.to_owned())
}
