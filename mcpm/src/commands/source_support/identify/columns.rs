//! Column selection for local file identification results.

use clap::{Args, ValueEnum};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum, Debug)]
pub(super) enum Column {
    Index,
    Path,
    Source,
    Project,
    Id,
    Filename,
    Size,
    Role,
    Status,
}

impl Column {
    const ALL: [Self; 9] = [
        Self::Index,
        Self::Path,
        Self::Source,
        Self::Project,
        Self::Id,
        Self::Filename,
        Self::Size,
        Self::Role,
        Self::Status,
    ];

    fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Path => 'P',
            Self::Source => 'O',
            Self::Project => 'J',
            Self::Id => 'F',
            Self::Filename => 'A',
            Self::Size => 'B',
            Self::Role => 'R',
            Self::Status => 'S',
        }
    }
}

#[derive(Args, Debug)]
pub(super) struct ColumnArgs {
    /// Select columns: I=index, P=path, O=source, J=project, F=id, A=filename, B=size, R=role, S=status.
    #[arg(short = 'C', value_name = "COLUMNS", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Comma-separated columns, combined with -C.
    #[arg(long, value_enum, value_delimiter = ',')]
    columns: Vec<Column>,
}

impl ColumnArgs {
    pub(super) fn selected(&self) -> Vec<Column> {
        Column::ALL
            .into_iter()
            .filter(|c| {
                (self.compact.is_empty() && self.columns.is_empty())
                    || self.columns.contains(c)
                    || self.compact.iter().any(|s| s.contains(c.letter()))
            })
            .collect()
    }
}

fn parse_letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|c| "IPOJFABRS".contains(c)) {
        return Err("expected column letters IPOJFABRS".into());
    }
    Ok(value.to_owned())
}
