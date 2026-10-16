//! Mod list rows, styles and rendering independent of command input.

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::Path;

use kako_mods_lib::local::modloader::ModLoaderMeta;
use owo_colors::Style;
use unicode_width::UnicodeWidthStr;

use super::super::{mod_records, source_path::PathArgs};
use super::{ListedFile, columns::Column};

mod index;
mod layout;
pub(super) use index::IndexType;

#[derive(Clone)]
struct Piece {
    text: String,
    style: Style,
}

impl Piece {
    fn new(value: impl AsRef<str>, style: Style) -> Self {
        Self {
            text: text(value.as_ref()),
            style,
        }
    }
}

struct Row {
    cells: [Vec<Piece>; 5],
    index_labels: index::Value,
    name_indent: usize,
}

impl Row {
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
                vec![Piece::new(name, Style::new().bright_white().bold())],
                vec![Piece::new(modid, Style::new().bright_black())],
                filename,
                loaders,
            ],
            index_labels: index,
            name_indent,
        }
    }

    fn width(&self, column: Column) -> usize {
        self.cells[column.position()]
            .iter()
            .map(|piece| piece.text.as_str())
            .collect::<String>()
            .width()
            + self.indentation(column)
    }

    fn indentation(&self, column: Column) -> usize {
        if column == Column::Name {
            self.name_indent
        } else {
            0
        }
    }
}

pub(super) struct ListOptions<'a> {
    pub expanded: bool,
    pub color: bool,
    pub columns: &'a [Column],
    pub width: Option<usize>,
    pub paths: &'a PathArgs,
    pub index_type: IndexType,
}

pub(super) fn write_list(
    output: &mut impl Write,
    files: &[ListedFile],
    options: ListOptions<'_>,
) -> io::Result<()> {
    let mut rows = Vec::new();
    for (index, listed) in files.iter().enumerate() {
        append_rows(&mut rows, listed, index + 1, &options);
    }
    index::align(&mut rows);
    layout::write(output, &rows, options.columns, options.color, options.width)
}

fn append_rows(rows: &mut Vec<Row>, listed: &ListedFile, number: usize, options: &ListOptions<'_>) {
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
        let mut loaders = vec![Piece::new("[unrecognized]", Style::new().bright_black())];
        if !listed.scanned.failures.is_empty() {
            loaders.push(failure_count(listed.scanned.failures.len()));
        }
        rows.push(Row::new(
            indices.file(number),
            file.entry.name(),
            "",
            filename(&[], file.entry.path(), options.paths),
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
                filename(&record.source, file.entry.path(), options.paths),
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
                filename(&[], file.entry.path(), options.paths),
                vec![
                    loader_piece(&failure.loader),
                    Piece::new(" [failed]", warning()),
                    Piece::new(format!(" {}", failure.reason), Style::new().bright_white()),
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
                Style::new().bright_black(),
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
            filename(&record.source, file.entry.path(), options.paths),
            loaders,
            if matches!(options.index_type, IndexType::Tree) {
                record.source.len() * 2
            } else {
                0
            },
        ));
    }
}

fn filename(source: &[String], outer: &Path, paths: &PathArgs) -> Vec<Piece> {
    paths
        .pieces(outer, source)
        .into_iter()
        .map(|(part, style)| Piece::new(part, style))
        .collect()
}

fn loader_piece(loader: &ModLoaderMeta) -> Piece {
    Piece::new(
        format!("[{}]", loader.name()),
        loader_style(loader.id().as_str()),
    )
}

fn loader_style(id: &str) -> Style {
    match id {
        "forge" => Style::new().bright_yellow(),
        "neoforge" => Style::new().bright_red(),
        "fabric" => Style::new().bright_cyan(),
        _ => Style::new().bright_blue(),
    }
}

fn warning() -> Style {
    Style::new().bright_yellow().bold()
}
fn space() -> Piece {
    Piece::new(" ", Style::new())
}
fn failure_count(count: usize) -> Piece {
    Piece::new(format!(" (+{count} failed)"), warning())
}

// Measure and print the same escaped text to keep columns aligned.
fn text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}
