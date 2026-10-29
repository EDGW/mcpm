//! Console and raw rendering for shared semantic mod rows.

use super::{
    columns::Column,
    rows::{Paint, Piece, Row},
};
use crate::theme;
use owo_colors::Style;
use std::io::{self, Write};

mod layout;

pub(super) fn write_list(
    output: &mut impl Write,
    rows: &[Row],
    columns: &[Column],
    color: bool,
    width: Option<usize>,
) -> io::Result<()> {
    layout::write(output, rows, columns, color, width)
}

fn paint(value: &Paint) -> Style {
    match value {
        Paint::Plain => Style::new(),
        Paint::Role(role) => theme::console(*role),
        Paint::Loader(id) => theme::get().loader(id),
    }
}
