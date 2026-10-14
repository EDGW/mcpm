use std::collections::HashSet;
use std::io::{self, Write};

use kako_mods_lib::local::{mcmod::ModInfo, modloader::ModLoaderMeta};
use owo_colors::Style;
use unicode_width::UnicodeWidthStr;

use super::{ListedFile, columns::Column};

mod layout;

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
}

impl Row {
    fn new(
        index: Option<usize>,
        name: &str,
        modid: &str,
        filename: &str,
        loaders: Vec<Piece>,
    ) -> Self {
        Self {
            cells: [
                vec![Piece::new(
                    index.map(|index| format!("[{index}]")).unwrap_or_default(),
                    Style::new(),
                )],
                vec![Piece::new(name, Style::new().bright_white().bold())],
                vec![Piece::new(modid, Style::new().bright_black())],
                vec![Piece::new(filename, Style::new().bright_black())],
                loaders,
            ],
        }
    }

    fn width(&self, column: Column) -> usize {
        self.cells[column.position()]
            .iter()
            .map(|piece| piece.text.as_str())
            .collect::<String>()
            .width()
    }
}

pub(super) fn write_list(
    output: &mut impl Write,
    files: &[ListedFile],
    expanded: bool,
    color: bool,
    columns: &[Column],
    width: Option<usize>,
) -> io::Result<()> {
    let mut rows = Vec::new();
    for (index, listed) in files.iter().enumerate() {
        append_rows(&mut rows, listed, index + 1, expanded);
    }
    layout::write(output, &rows, columns, color, width)
}

fn append_rows(rows: &mut Vec<Row>, listed: &ListedFile, number: usize, expanded: bool) {
    let file = &listed.file;
    let records: Vec<_> = file
        .metadata
        .iter()
        .flat_map(|metadata| metadata.mods.iter().map(|info| (&metadata.loader, info)))
        .collect();
    let outer_filename = file
        .entry
        .path()
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    if records.is_empty() {
        let mut loaders = vec![Piece::new("[unrecognized]", Style::new().bright_black())];
        if !listed.failures.is_empty() {
            loaders.push(failure_count(listed.failures.len()));
        }
        rows.push(Row::new(
            Some(number),
            file.entry.name(),
            "",
            &outer_filename,
            loaders,
        ));
    }
    if expanded {
        for (index, (loader, info)) in records.iter().enumerate() {
            rows.push(Row::new(
                (index == 0).then_some(number),
                &info.name,
                &info.modid,
                &filename(info, &outer_filename),
                vec![loader_piece(loader)],
            ));
        }
        for failure in &listed.failures {
            rows.push(Row::new(
                None,
                file.entry.name(),
                "",
                &outer_filename,
                vec![
                    loader_piece(&failure.loader),
                    Piece::new(" [failed]", warning()),
                    Piece::new(format!(" {}", failure.reason), Style::new().bright_white()),
                ],
            ));
        }
    } else if let Some((loader, info)) = records.first() {
        let mut loaders = vec![loader_piece(loader)];
        if records.len() > 1 {
            loaders.push(Piece::new(
                format!(" (+{} mods)", records.len() - 1),
                Style::new().bright_black(),
            ));
        }
        let mut displayed = HashSet::from([loader.id.as_str()]);
        for (loader, _) in records.iter().skip(1) {
            if displayed.insert(loader.id.as_str()) {
                loaders.push(space());
                loaders.push(loader_piece(loader));
            }
        }
        if !listed.failures.is_empty() {
            loaders.push(space());
            for (index, failure) in listed.failures.iter().enumerate() {
                if index > 0 {
                    loaders.push(space());
                }
                loaders.push(loader_piece(&failure.loader));
            }
            loaders.push(failure_count(listed.failures.len()));
        }
        rows.push(Row::new(
            Some(number),
            &info.name,
            &info.modid,
            &filename(info, &outer_filename),
            loaders,
        ));
    }
}

fn filename(info: &ModInfo, outer: &str) -> String {
    match info.nested_path.last() {
        Some(member) => format!("nested {}", member.rsplit('/').next().unwrap_or(member)),
        None => outer.to_owned(),
    }
}

fn loader_piece(loader: &ModLoaderMeta) -> Piece {
    let style = match loader.id.as_str() {
        "forge" => Style::new().bright_yellow(),
        "neoforge" => Style::new().bright_red(),
        "fabric" => Style::new().bright_cyan(),
        "quilt" => Style::new().bright_magenta(),
        _ => Style::new().bright_blue(),
    };
    Piece::new(format!("[{}]", loader.name), style)
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
