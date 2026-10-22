//! Shared semantic mod rows used by interactive and line-oriented renderers.

use std::collections::HashSet;
use std::ops::Range;

use crate::theme::Role;
use kako_mods_lib::local::mcmod::ModEntry;
use kako_mods_lib::local::modloader::ModLoaderMeta;
use unicode_width::UnicodeWidthStr;

use super::super::{mod_records, source_path::PathArgs};
use super::{ListedFile, columns::Column};

mod index;
pub(super) use index::{IndexType, align};

#[derive(Clone, Debug)]
pub(super) struct Piece {
    pub(super) text: String,
    pub(super) style: Paint,
    /// Semantic value within a decorated fragment, such as a loader name.
    pub(super) value: Option<Range<usize>>,
}

impl Piece {
    pub(super) fn new(value: impl AsRef<str>, style: Paint) -> Self {
        Self {
            text: crate::commands::output_style::text(value.as_ref()),
            style,
            value: None,
        }
    }
}

#[derive(Debug)]
pub(super) struct Row {
    pub(super) cells: [Vec<Piece>; 5],
    index_labels: index::Value,
    name_indent: usize,
}

impl Row {
    /// Return searchable values and their positions in the complete displayed field.
    pub(super) fn searchable(&self, column: Column) -> (String, Vec<Range<usize>>) {
        let mut text = String::new();
        let mut values = Vec::new();
        for piece in &self.cells[column.position()] {
            if let Some(value) = &piece.value {
                values.push(text.len() + value.start..text.len() + value.end);
            }
            text.push_str(&piece.text);
        }
        if values.is_empty() {
            values.push(0..text.len());
        }
        (text, values)
    }

    fn new(
        index: index::Value,
        name: &str,
        modid: &str,
        filename: Vec<Piece>,
        loaders: Vec<Piece>,
        name_indent: usize,
    ) -> Self {
        Self {
            cells: [
                Vec::new(),
                vec![Piece::new(name, Paint::Role(Role::Primary))],
                vec![Piece::new(modid, Paint::Role(Role::Important))],
                filename,
                loaders,
            ],
            index_labels: index,
            name_indent,
        }
    }

    pub(super) fn width(&self, column: Column) -> usize {
        self.cells[column.position()]
            .iter()
            .map(|piece| piece.text.as_str())
            .collect::<String>()
            .width()
            + self.indentation(column)
    }

    pub(super) fn indentation(&self, column: Column) -> usize {
        if column == Column::Name {
            self.name_indent
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub(super) struct RowOptions<'a> {
    pub expanded: bool,
    pub paths: &'a PathArgs,
    pub index_type: IndexType,
}

pub(super) fn collect(files: &[ListedFile], options: &RowOptions<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, listed) in files.iter().enumerate() {
        append_rows(&mut rows, listed, index + 1, options);
    }
    index::align(&mut rows);
    rows
}

/// Build one file's rows with its original position preserved for interactive trees.
pub(super) fn for_file(file: &ListedFile, number: usize, options: &RowOptions<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    append_rows(&mut rows, file, number, options);
    index::align(&mut rows);
    rows
}

fn append_rows(rows: &mut Vec<Row>, listed: &ListedFile, number: usize, options: &RowOptions<'_>) {
    let file = &listed.scanned.file;
    let file_index = number.to_string();
    let multiple_loaders = file.metadata.len() + listed.scanned.failures.len() > 1;
    let mut indices = index::Formatter::new(options.index_type);
    let records: Vec<_> = file
        .metadata
        .iter()
        .enumerate()
        .flat_map(|(metadata_index, metadata)| {
            let prefix = if !multiple_loaders {
                file_index.clone()
            } else {
                format!("{file_index}-{}", metadata.loader.id().as_str())
            };
            mod_records::selected(metadata, metadata_index, &prefix, listed.records.as_deref())
        })
        .collect();
    if records.is_empty() {
        let mut loaders = vec![Piece::new("[unrecognized]", Paint::Role(Role::Muted))];
        if !listed.scanned.failures.is_empty() {
            loaders.push(failure_count(listed.scanned.failures.len()));
        }
        rows.push(Row::new(
            indices.file(number),
            file.entry.name(),
            "",
            filename(&[], &file.entry, options.paths),
            loaders,
            0,
        ));
    }
    if options.expanded {
        for record in &records {
            let info = record.info;
            rows.push(Row::new(
                indices.record(record, number, multiple_loaders),
                &info.name,
                &info.modid,
                filename(&record.source, &file.entry, options.paths),
                vec![loader_piece(record.loader)],
                if matches!(options.index_type, IndexType::Tree) {
                    record.source.len() * 2
                } else {
                    0
                },
            ));
        }
        for failure in &listed.scanned.failures {
            rows.push(Row::new(
                indices.failure(number, failure.loader.id().as_str()),
                file.entry.name(),
                "",
                filename(&[], &file.entry, options.paths),
                vec![
                    loader_piece(&failure.loader),
                    Piece::new(" [failed]", warning()),
                    Piece::new(format!(" {}", failure.reason), Paint::Role(Role::Normal)),
                ],
                0,
            ));
        }
    } else if let Some(record) = records.first() {
        let loader = record.loader;
        let info = record.info;
        let mut loaders = vec![loader_piece(loader)];
        if records.len() > 1 {
            loaders.push(Piece::new(
                format!(" (+{} mods)", records.len() - 1),
                Paint::Role(Role::Muted),
            ));
        }
        let mut displayed = HashSet::from([loader.id().as_str()]);
        for record in records.iter().skip(1) {
            let loader = record.loader;
            if displayed.insert(loader.id().as_str()) {
                loaders.push(space());
                loaders.push(loader_piece(loader));
            }
        }
        if !listed.scanned.failures.is_empty() {
            loaders.push(space());
            for (index, failure) in listed.scanned.failures.iter().enumerate() {
                if index > 0 {
                    loaders.push(space());
                }
                loaders.push(loader_piece(&failure.loader));
            }
            loaders.push(failure_count(listed.scanned.failures.len()));
        }
        rows.push(Row::new(
            indices.record(record, number, multiple_loaders),
            &info.name,
            &info.modid,
            filename(&record.source, &file.entry, options.paths),
            loaders,
            if matches!(options.index_type, IndexType::Tree) {
                record.source.len() * 2
            } else {
                0
            },
        ));
    }
}

fn filename(source: &[String], entry: &ModEntry, paths: &PathArgs) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (index, part) in paths
        .parts_with_filename(entry.path(), source, entry.filename())
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            pieces.push(Piece::new("!", Paint::Role(Role::Important)));
        }
        pieces.push(Piece::new(part, Paint::Role(Role::Muted)));
    }
    pieces
}

fn loader_piece(loader: &ModLoaderMeta) -> Piece {
    let mut piece = Piece::new(
        format!("[{}]", loader.name()),
        loader_style(loader.id().as_str()),
    );
    piece.value = Some(1..piece.text.len() - 1);
    piece
}

fn loader_style(id: &str) -> Paint {
    Paint::Loader(id.to_owned())
}

fn warning() -> Paint {
    Paint::Role(Role::Warning)
}
fn space() -> Piece {
    Piece::new(" ", Paint::Plain)
}
fn failure_count(count: usize) -> Piece {
    Piece::new(format!(" (+{count} failed)"), warning())
}

/// Semantic paint is resolved independently by each renderer and its active palette.
#[derive(Clone, Default, Debug)]
pub(super) enum Paint {
    #[default]
    Plain,
    Role(Role),
    Loader(String),
}
