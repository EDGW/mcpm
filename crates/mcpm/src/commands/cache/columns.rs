//! Cache column selection with explicit override of command-specific defaults.

use clap::Args;

use crate::display::cache::Column;

#[derive(Debug, Args)]
pub(super) struct Columns {
    /// Select columns: B=blake3, F=file, S=size, H=sha256, T=status, M=metadatas.
    #[arg(short = 'C', value_name = "BFSHTM", value_parser = letters)]
    compact: Vec<String>,
    /// Select comma-separated columns, combined with -C and --column.
    #[arg(long, value_enum, value_delimiter = ',')]
    columns: Vec<Column>,
    /// Select a column; repeat to select more.
    #[arg(long, value_enum)]
    column: Vec<Column>,
}

impl Columns {
    pub(super) fn explicit(&self) -> bool {
        !self.compact.is_empty() || !self.columns.is_empty() || !self.column.is_empty()
    }

    pub(super) fn selected(&self, default: &[Column]) -> Vec<Column> {
        if !self.explicit() {
            return default.to_vec();
        }
        Column::ALL
            .into_iter()
            .filter(|column| {
                self.columns.contains(column)
                    || self.column.contains(column)
                    || self
                        .compact
                        .iter()
                        .any(|letters| letters.contains(column.letter()))
            })
            .collect()
    }
}

fn letters(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|ch| "BFSHTM".contains(ch)) {
        return Err("expected column letters BFSHTM".into());
    }
    Ok(value.into())
}
