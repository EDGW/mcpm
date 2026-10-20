//! Column selection for list output.

use clap::{Args, ValueEnum};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum Column {
    Index,
    Selected,
    Source,
    Project,
    Id,
    Name,
    Version,
    Release,
    GameVersions,
    Loaders,
    Filename,
    Size,
    Url,
}

impl Column {
    const ALL: [Self; 13] = [
        Self::Index,
        Self::Selected,
        Self::Source,
        Self::Project,
        Self::Id,
        Self::Name,
        Self::Version,
        Self::Release,
        Self::GameVersions,
        Self::Loaders,
        Self::Filename,
        Self::Size,
        Self::Url,
    ];
    fn letter(self) -> char {
        match self {
            Self::Index => 'I',
            Self::Selected => 'S',
            Self::Source => 'O',
            Self::Project => 'P',
            Self::Id => 'F',
            Self::Name => 'N',
            Self::Version => 'V',
            Self::Release => 'R',
            Self::GameVersions => 'G',
            Self::Loaders => 'L',
            Self::Filename => 'A',
            Self::Size => 'B',
            Self::Url => 'U',
        }
    }
}

#[derive(Args)]
pub(super) struct ColumnArgs {
    /// Select list columns: I=index, S=selected, O=source, P=project, F=id, N=name, V=version, R=release, G=gameversions, L=loaders, A=filename, B=size, U=url.
    #[arg(short = 'C', value_name = "COLUMNS", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Comma-separated columns, combined with -C. Selecting columns disables TUI.
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
    if value.is_empty() || !value.chars().all(|c| "ISOPFNVRGLABU".contains(c)) {
        return Err("expected column letters ISOPFNVRGLABU".into());
    }
    Ok(value.to_owned())
}
