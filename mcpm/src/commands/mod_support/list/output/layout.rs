//! Width-aware terminal layout for mod list rows.

use std::io::{self, Write};

use textwrap::{Options, WordSeparator, WordSplitter, WrapAlgorithm};
use unicode_width::UnicodeWidthStr;

use super::{Column, Piece, Row};

pub(super) fn write(
    output: &mut impl Write,
    rows: &[Row],
    columns: &[Column],
    color: bool,
    width: Option<usize>,
) -> io::Result<()> {
    let Some(width) = width else {
        for row in rows {
            for (index, column) in columns.iter().enumerate() {
                if index > 0 {
                    write!(output, "\t")?;
                }
                write!(output, "{}", " ".repeat(row.indentation(*column)))?;
                write_pieces(output, &row.cells[column.position()], false)?;
            }
            writeln!(output)?;
        }
        return Ok(());
    };
    if let Some(widths) = allocate(rows, columns, width) {
        for row in rows {
            write_table_row(output, row, columns, &widths, color)?;
        }
    } else {
        for row in rows {
            write_stacked_row(output, row, columns, width, color)?;
        }
    }
    Ok(())
}

fn allocate(rows: &[Row], columns: &[Column], width: usize) -> Option<[usize; 5]> {
    let mut desired = [0; 5];
    for row in rows {
        for &column in columns {
            let position = column.position();
            desired[position] = desired[position].max(row.width(column));
        }
    }
    let minimum = [desired[0], 8, 10, 12, 14];
    let maximum = [desired[0], 40, 32, 48, 40];
    let mut widths = [0; 5];
    for &column in columns {
        let position = column.position();
        desired[position] = desired[position].min(maximum[position]);
        widths[position] = desired[position].min(minimum[position]);
    }
    let available = width.checked_sub(columns.len().saturating_sub(1) * 2)?;
    let mut used = widths.iter().sum::<usize>();
    if used > available {
        return None;
    }
    // Give names extra room while allowing every visible column to grow.
    while used < available {
        let mut grew = false;
        for column in [
            Column::Name,
            Column::Name,
            Column::Filename,
            Column::Modid,
            Column::Loaders,
        ] {
            let position = column.position();
            if used < available && columns.contains(&column) && widths[position] < desired[position]
            {
                widths[position] += 1;
                used += 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    Some(widths)
}

fn write_table_row(
    output: &mut impl Write,
    row: &Row,
    columns: &[Column],
    widths: &[usize; 5],
    color: bool,
) -> io::Result<()> {
    let cells: Vec<_> = columns
        .iter()
        .map(|&column| {
            wrap_cell(
                &row.cells[column.position()],
                widths[column.position()],
                column,
                row.indentation(column),
            )
        })
        .collect();
    let height = cells.iter().map(Vec::len).max().unwrap_or(1);
    for line in 0..height {
        for (position, &column) in columns.iter().enumerate() {
            let pieces = cells[position].get(line).map(Vec::as_slice).unwrap_or(&[]);
            let padding = widths[column.position()].saturating_sub(piece_width(pieces));
            write_pieces(output, pieces, color)?;
            if position + 1 < columns.len() {
                write!(output, "{}", " ".repeat(padding + 2))?;
            }
        }
        writeln!(output)?;
    }
    Ok(())
}

fn write_stacked_row(
    output: &mut impl Write,
    row: &Row,
    columns: &[Column],
    width: usize,
    color: bool,
) -> io::Result<()> {
    for &column in columns {
        let cell = &row.cells[column.position()];
        if cell.iter().all(|piece| piece.text.is_empty()) {
            continue;
        }
        let label = match column {
            Column::Index => "",
            Column::Name => "name: ",
            Column::Modid => "modid: ",
            Column::Filename => "filename: ",
            Column::Loaders => "loaders: ",
        };
        let indentation = if column != Column::Index && width > 3 {
            "  "
        } else {
            ""
        };
        let prefix = format!("{indentation}{label}");
        let minimum_content = if column == Column::Loaders { 14 } else { 2 };
        if width.saturating_sub(prefix.len()) >= minimum_content {
            for (index, line) in
                wrap_cell(cell, width - prefix.len(), column, row.indentation(column))
                    .iter()
                    .enumerate()
            {
                write!(
                    output,
                    "{}",
                    if index == 0 {
                        prefix.clone()
                    } else {
                        " ".repeat(prefix.len())
                    }
                )?;
                write_pieces(output, line, color)?;
                writeln!(output)?;
            }
        } else {
            // Extremely narrow terminals: put labels on their own wrapped lines.
            for line in textwrap::wrap(prefix.trim_end(), width.max(1)) {
                if !line.is_empty() {
                    writeln!(output, "{line}")?;
                }
            }
            for line in wrap_cell(
                cell,
                width.saturating_sub(indentation.len()).max(1),
                column,
                row.indentation(column),
            ) {
                write!(output, "{indentation}")?;
                write_pieces(output, &line, color)?;
                writeln!(output)?;
            }
        }
    }
    writeln!(output)
}

fn wrap_cell(
    pieces: &[Piece],
    width: usize,
    column: Column,
    indentation: usize,
) -> Vec<Vec<Piece>> {
    let indentation = indentation.min(width.saturating_sub(1));
    let text: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
    let mut options = Options::new(width.saturating_sub(indentation).max(1))
        .word_splitter(WordSplitter::NoHyphenation)
        .wrap_algorithm(WrapAlgorithm::FirstFit);
    if column == Column::Loaders {
        options = options.word_separator(WordSeparator::AsciiSpace);
    }
    let mut cursor = 0;
    textwrap::wrap(&text, options)
        .into_iter()
        .map(|line| {
            // Wrapping uses plain text; apply the original styles to each resulting slice.
            let start = cursor + text[cursor..].find(line.as_ref()).unwrap_or(0);
            let end = start + line.len();
            cursor = end;
            let mut offset = 0;
            let mut result = Vec::new();
            if indentation > 0 {
                result.push(Piece::new(
                    " ".repeat(indentation),
                    owo_colors::Style::new(),
                ));
            }
            for piece in pieces {
                let piece_end = offset + piece.text.len();
                let slice_start = start.max(offset);
                let slice_end = end.min(piece_end);
                if slice_start < slice_end {
                    result.push(Piece {
                        text: piece.text[slice_start - offset..slice_end - offset].to_owned(),
                        style: piece.style,
                    });
                }
                offset = piece_end;
            }
            result
        })
        .collect()
}

fn piece_width(pieces: &[Piece]) -> usize {
    pieces
        .iter()
        .map(|piece| piece.text.as_str())
        .collect::<String>()
        .width()
}

fn write_pieces(output: &mut impl Write, pieces: &[Piece], color: bool) -> io::Result<()> {
    for piece in pieces {
        if color {
            write!(output, "{}", piece.style.style(&piece.text))?;
        } else {
            write!(output, "{}", piece.text)?;
        }
    }
    Ok(())
}
