//! Scoped filter parsing shared by matching and syntax highlighting; indices are excluded.

use super::super::{columns::Column, rows::Row};
use std::{borrow::Cow, ops::Range};

pub(super) const FIELDS: [Column; 4] = [
    Column::Name,
    Column::Modid,
    Column::Filename,
    Column::Loaders,
];

pub(super) struct Query<'a> {
    pub parts: Vec<Part<'a>>,
}
pub(super) struct Part<'a> {
    /// None is the unscoped prefix; Some marks an @ directive, even if invalid.
    pub marker: Option<Range<usize>>,
    pub content: Range<usize>,
    pub field: Option<Column>,
    value: Cow<'a, str>,
}
impl<'a> Query<'a> {
    pub fn parse(text: &'a str) -> Self {
        let mut parts = Vec::new();
        let mut starts = markers(text).into_iter().peekable();
        let prefix = starts.peek().copied().unwrap_or(text.len());
        if prefix > 0 {
            parts.push(Part {
                marker: None,
                content: 0..prefix,
                field: None,
                value: literal(&text[..prefix]),
            });
        }
        while let Some(start) = starts.next() {
            let end = starts.peek().copied().unwrap_or(text.len());
            let marker_end = text[start..end]
                .find(char::is_whitespace)
                .map_or(end, |offset| start + offset);
            let field = match text[start..marker_end].to_ascii_lowercase().as_str() {
                "@name" => Some(Column::Name),
                "@modid" => Some(Column::Modid),
                "@file" => Some(Column::Filename),
                "@loaders" => Some(Column::Loaders),
                _ => None,
            };
            parts.push(Part {
                marker: Some(start..marker_end),
                content: marker_end..end,
                field,
                value: literal(&text[marker_end..end]),
            });
        }
        Self { parts }
    }

    /// All clauses must match; an unscoped clause may match any supported field.
    pub fn matches(&self, row: &Row) -> bool {
        self.parts.iter().all(|part| {
            if part.marker.is_some() && part.field.is_none() {
                return false;
            }
            let value = part.value.trim().to_lowercase();
            if value.is_empty() {
                return true;
            }
            let matches = |column: Column| {
                row.cells[column.position()]
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>()
                    .to_lowercase()
                    .contains(&value)
            };
            part.field
                .map_or_else(|| FIELDS.into_iter().any(matches), matches)
        })
    }

    pub fn invalid(&self) -> bool {
        self.parts
            .iter()
            .any(|part| part.marker.is_some() && part.field.is_none())
    }
}

/// Only a backslash directly followed by @ is escaped; other path characters stay literal.
fn markers(text: &str) -> Vec<usize> {
    let mut chars = text.char_indices().peekable();
    let mut starts = Vec::new();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|(_, next)| *next == '@') {
            chars.next();
        } else if ch == '@' {
            starts.push(index);
        }
    }
    starts
}

fn literal(text: &str) -> Cow<'_, str> {
    if text.contains("\\@") {
        Cow::Owned(text.replace("\\@", "@"))
    } else {
        Cow::Borrowed(text)
    }
}
