//! Flat and tree indices for mod list records.

use crate::theme::Role;
use clap::ValueEnum;
use unicode_width::UnicodeWidthStr;

use super::super::super::mod_records::ModRecord;
use super::{Paint, Piece, Row, loader_style};

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum IndexType {
    /// Full declaration indices, such as [1-neoforge+2-3].
    Flat,
    /// Align the first item after each file number; indent deeper levels compactly.
    Tree,
}

pub(super) enum Value {
    Flat(Vec<Piece>),
    Tree {
        parts: Vec<String>,
        shared: usize,
        loader: Option<Paint>,
    },
}

/// Anchor the first item after the file number; deeper ancestors use two spaces.
pub(in crate::commands::mod_support::list) fn align(rows: &mut [Row]) {
    // Filtering may remove the row that originally supplied a shared prefix.
    // Recalculate sharing from the rows that will actually be displayed.
    let mut previous = Vec::new();
    for row in rows.iter_mut() {
        if let Value::Tree { parts, shared, .. } = &mut row.index_labels {
            *shared = previous
                .iter()
                .zip(parts.iter())
                .take_while(|(left, right)| left == right)
                .count();
            previous.clone_from(parts);
        }
    }
    let file_width = rows
        .iter()
        .filter_map(|row| match &row.index_labels {
            Value::Tree { parts, .. } => parts.first().map(|part| part.width()),
            Value::Flat(_) => None,
        })
        .max()
        .unwrap_or(0);
    for row in rows {
        let value = match &row.index_labels {
            Value::Flat(pieces) => pieces.clone(),
            Value::Tree {
                parts,
                shared,
                loader,
            } => {
                let mut pieces = Vec::new();
                if *shared == 0 {
                    pieces.push(Piece::new(&parts[0], Paint::Plain));
                    if parts.len() > 1 {
                        pieces.push(Piece::new(
                            " ".repeat(file_width - parts[0].width()),
                            Paint::Plain,
                        ));
                    }
                } else {
                    let guides = format!(
                        "·{}{}",
                        " ".repeat(file_width.saturating_sub(1)),
                        "· ".repeat(shared - 1),
                    );
                    pieces.push(Piece::new(guides, Paint::Role(Role::Muted)));
                }
                for (position, part) in parts.iter().enumerate().skip((*shared).max(1)) {
                    let style = if position == 1 {
                        loader.clone().unwrap_or_default()
                    } else {
                        Paint::Plain
                    };
                    pieces.push(Piece::new(part, style));
                }
                pieces
            }
        };
        row.cells[0] = value;
    }
}

pub(super) struct Formatter {
    kind: IndexType,
    previous: Vec<String>,
}

impl Formatter {
    pub(super) fn new(kind: IndexType) -> Self {
        Self {
            kind,
            previous: Vec::new(),
        }
    }

    pub(super) fn record(
        &mut self,
        record: &ModRecord<'_>,
        file: usize,
        multiple_loaders: bool,
    ) -> Value {
        if matches!(self.kind, IndexType::Flat) {
            if !multiple_loaders {
                return Value::Flat(vec![Piece::new(
                    format!("[{}]", record.index),
                    Paint::Plain,
                )]);
            }
            let mut suffix = record
                .nested_indices
                .iter()
                .map(|index| format!("+{index}"))
                .collect::<String>();
            if let Some(index) = record.native_index {
                suffix.push_str(&format!("-{index}"));
            }
            return Value::Flat(vec![
                Piece::new(format!("[{file}-"), Paint::Plain),
                Piece::new(
                    record.loader.id().as_str(),
                    loader_style(record.loader.id().as_str()),
                ),
                Piece::new(format!("{suffix}]"), Paint::Plain),
            ]);
        }
        let mut parts = vec![format!("[{file}]")];
        if multiple_loaders {
            parts.push(format!("[{}]", record.loader.id().as_str()));
        }
        parts.extend(
            record
                .nested_indices
                .iter()
                .map(|index| format!("<{index}>")),
        );
        if let Some(index) = record.native_index {
            parts.push(format!("[{index}]"));
        }
        self.tree(
            parts,
            multiple_loaders.then(|| loader_style(record.loader.id().as_str())),
        )
    }

    pub(super) fn file(&mut self, file: usize) -> Value {
        match self.kind {
            IndexType::Flat => Value::Flat(vec![Piece::new(format!("[{file}]"), Paint::Plain)]),
            IndexType::Tree => self.tree(vec![format!("[{file}]")], None),
        }
    }

    pub(super) fn failure(&mut self, file: usize, loader: &str) -> Value {
        match self.kind {
            IndexType::Flat => Value::Flat(vec![
                Piece::new(format!("[{file}-"), Paint::Plain),
                Piece::new(loader, loader_style(loader)),
                Piece::new("]", Paint::Plain),
            ]),
            IndexType::Tree => self.tree(
                vec![format!("[{file}]"), format!("[{loader}]")],
                Some(loader_style(loader)),
            ),
        }
    }

    fn tree(&mut self, parts: Vec<String>, loader: Option<Paint>) -> Value {
        let shared = self
            .previous
            .iter()
            .zip(&parts)
            .take_while(|(previous, current)| previous == current)
            .count();
        self.previous = parts.clone();
        Value::Tree {
            parts,
            shared,
            loader,
        }
    }
}
