//! Command-specific Clap column sets sharing one selection and validation implementation.

use anyhow::{Result, ensure};
use clap::{
    Args,
    builder::{PossibleValuesParser, TypedValueParser},
};

use crate::display::stash::Column;

/// List exposes only index fields; its Clap parser and compact letters use the same field set.
#[derive(Debug, Args)]
pub(super) struct ListColumns {
    /// Select columns by letters: N=name, I=id, T=time, H=head, B=backup.
    #[arg(short = 'C', value_name = "NITHB", value_parser = list_letters)]
    compact: Vec<String>,
    /// Select comma-separated columns, combined with -C and --column.
    #[arg(long, value_parser = list_values(), value_delimiter = ',')]
    columns: Vec<Column>,
    /// Select one column; repeat to select more.
    #[arg(long, value_parser = list_values())]
    column: Vec<Column>,
}

impl ListColumns {
    pub fn explicit(&self) -> bool {
        explicit(&self.compact, &self.columns, &self.column)
    }

    pub fn selected(&self, details: bool) -> Result<Vec<Column>> {
        selected(
            &self.compact,
            &self.columns,
            &self.column,
            &Column::LIST,
            if details {
                &Column::LIST
            } else {
                &[Column::Name, Column::Time, Column::Head]
            },
        )
    }
}

/// Info exposes all summary fields independently of its per-file details option.
#[derive(Debug, Args)]
pub(super) struct InfoColumns {
    /// Select columns by letters: N=name, I=id, T=time, H=head, B=backup, P=path, C=checksum, F=files, S=size.
    #[arg(short = 'C', value_name = "NITHBPCFS", value_parser = info_letters)]
    compact: Vec<String>,
    /// Select comma-separated columns, combined with -C and --column.
    #[arg(long, value_enum, value_delimiter = ',')]
    columns: Vec<Column>,
    /// Select one column; repeat to select more.
    #[arg(long, value_enum)]
    column: Vec<Column>,
}

impl InfoColumns {
    pub fn selected(&self) -> Result<Vec<Column>> {
        selected(
            &self.compact,
            &self.columns,
            &self.column,
            &Column::ALL,
            &Column::ALL,
        )
    }
}

fn selected(
    compact: &[String],
    columns: &[Column],
    column: &[Column],
    available: &[Column],
    defaults: &[Column],
) -> Result<Vec<Column>> {
    if !explicit(compact, columns, column) {
        return Ok(defaults.to_vec());
    }
    let selected: Vec<_> = available
        .iter()
        .copied()
        .filter(|choice| {
            columns.contains(choice)
                || column.contains(choice)
                || compact
                    .iter()
                    .any(|letters| letters.contains(choice.letter()))
        })
        .collect();
    ensure!(!selected.is_empty(), "at least one column is required");
    Ok(selected)
}

fn explicit(compact: &[String], columns: &[Column], column: &[Column]) -> bool {
    !compact.is_empty() || !columns.is_empty() || !column.is_empty()
}

fn list_values() -> impl TypedValueParser<Value = Column> {
    PossibleValuesParser::new(Column::LIST.map(Column::name)).map(|value| {
        Column::LIST
            .into_iter()
            .find(|column| column.name() == value)
            .expect("parser accepted a declared column")
    })
}

fn list_letters(value: &str) -> Result<String, String> {
    letters(value, &Column::LIST)
}
fn info_letters(value: &str) -> Result<String, String> {
    letters(value, &Column::ALL)
}

fn letters(value: &str, available: &[Column]) -> Result<String, String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| available.iter().any(|column| column.letter() == ch))
    {
        return Err(format!(
            "expected column letters {}",
            available
                .iter()
                .map(|column| column.letter())
                .collect::<String>()
        ));
    }
    Ok(value.into())
}
