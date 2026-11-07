//! Column selection for local file identification results.

use clap::Args;

pub(super) use crate::display::source_match::Column;

#[derive(Args, Debug)]
pub(super) struct ColumnArgs {
    /// Columns: I=index, P=path, O=source, J=project, F=id, A=filename, B=size, R=role, S=status; Q=query_status, C=cache_coverage, G=origin are optional.
    #[arg(short = 'C', value_name = "COLUMNS", value_parser = parse_letters)]
    compact: Vec<String>,
    /// Comma-separated columns, combined with -C and --column.
    #[arg(long, value_enum, value_delimiter = ',')]
    columns: Vec<Column>,
    /// Select one column; repeat to select multiple columns.
    #[arg(long, value_enum)]
    column: Vec<Column>,
}

impl ColumnArgs {
    pub(super) fn selected(&self) -> Vec<Column> {
        Column::ALL
            .into_iter()
            .chain(Column::OPTIONAL)
            .filter(|c| {
                (self.compact.is_empty()
                    && self.columns.is_empty()
                    && self.column.is_empty()
                    && Column::ALL.contains(c))
                    || self.columns.contains(c)
                    || self.column.contains(c)
                    || self.compact.iter().any(|s| s.contains(c.letter()))
            })
            .collect()
    }
}

fn parse_letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|c| "IPOJFABRSQCG".contains(c)) {
        return Err("expected column letters IPOJFABRSQCG".into());
    }
    Ok(value.to_owned())
}
