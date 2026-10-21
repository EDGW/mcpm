//! Scoped filter parsing shared by matching and syntax highlighting; indices are excluded.

use super::super::{columns::Column, rows::Row};
use std::{borrow::Cow, ops::Range};

pub(super) const FIELDS: [Column; 4] = [
    Column::Name,
    Column::Modid,
    Column::Filename,
    Column::Loaders,
];

pub(super) struct Query {
    pub parts: Vec<Part>,
}
pub(super) struct Part {
    /// None is the unscoped prefix; Some marks an @ directive, even if invalid.
    pub marker: Option<Range<usize>>,
    pub content: Range<usize>,
    pub field: Option<Column>,
    value: String,
}
impl Query {
    pub fn parse(text: &str) -> Self {
        let mut parts = Vec::new();
        let mut starts = markers(text).into_iter().peekable();
        let prefix = starts.peek().copied().unwrap_or(text.len());
        if prefix > 0 {
            parts.push(Part {
                marker: None,
                content: 0..prefix,
                field: None,
                value: literal(&text[..prefix]).trim().to_lowercase(),
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
                value: literal(&text[marker_end..end]).trim().to_lowercase(),
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
            let value = &part.value;
            if value.is_empty() {
                return true;
            }
            let matches = |column: Column| {
                row.cells[column.position()]
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>()
                    .to_lowercase()
                    .contains(value)
            };
            part.field
                .map_or_else(|| FIELDS.into_iter().any(matches), matches)
        })
    }

    /// Original UTF-8 byte ranges for the same literal clauses used by matching.
    /// Field scopes apply here too; ancestor context never highlights unrelated fields.
    pub fn ranges(&self, column: Column, text: &str) -> Vec<Range<usize>> {
        if !FIELDS.contains(&column)
            || self.invalid()
            || !self.parts.iter().any(|part| {
                !part.value.is_empty() && part.field.is_none_or(|field| field == column)
            })
        {
            return Vec::new();
        }
        let folded = text.to_lowercase();
        // Lowercasing can expand a scalar (e.g. İ -> i + combining dot). Map
        // folded byte boundaries back to whole original scalars before rendering.
        let mut offset = 0;
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(start, ch)| {
                let length = ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                let folded = offset..offset + length;
                offset += length;
                (folded, start..start + ch.len_utf8())
            })
            .collect();
        let mut ranges = Vec::new();
        for part in &self.parts {
            if part.value.is_empty() || part.field.is_some_and(|field| field != column) {
                continue;
            }
            for (start, value) in folded.match_indices(&part.value) {
                let first = boundaries.partition_point(|(folded, _)| folded.end <= start);
                let last =
                    boundaries.partition_point(|(folded, _)| folded.start < start + value.len());
                ranges.push(boundaries[first].1.start..boundaries[last - 1].1.end);
            }
        }
        ranges.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in ranges {
            if let Some(previous) = merged.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        merged
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_ranges_share_scopes_escaping_and_case_rules_with_filtering() {
        let query = Query::parse(r"@name API @file user\@HOST.jar");
        assert_eq!(
            query.ranges(Column::Name, "Fabric API api"),
            vec![7..10, 11..14]
        );
        assert!(query.ranges(Column::Name, "user@host.jar").is_empty());
        assert_eq!(
            query.ranges(Column::Filename, "mods/user@host.jar"),
            std::iter::once(5..18).collect::<Vec<_>>()
        );
        assert!(query.ranges(Column::Index, "API").is_empty());
        assert!(
            Query::parse("@invalid API")
                .ranges(Column::Name, "API")
                .is_empty()
        );
        assert!(
            Query::parse("@name ")
                .ranges(Column::Name, "API")
                .is_empty()
        );
        assert_eq!(
            Query::parse("api").ranges(Column::Loaders, "[API]"),
            std::iter::once(1..4).collect::<Vec<_>>()
        );
        assert_eq!(
            Query::parse("@name banana @name ana").ranges(Column::Name, "banana"),
            std::iter::once(0..6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn lowercase_expansion_and_context_map_back_to_original_utf8() {
        let text = "préİ 中文 ΟΣ i";
        let ranges = Query::parse("i").ranges(Column::Name, text);
        assert_eq!(
            ranges
                .iter()
                .map(|range| &text[range.clone()])
                .collect::<Vec<_>>(),
            vec!["İ", "i"]
        );
        let ranges = Query::parse("@name ος").ranges(Column::Name, text);
        assert_eq!(&text[ranges[0].clone()], "ΟΣ");
        let ranges = Query::parse("@file 中文").ranges(Column::Filename, text);
        assert_eq!(&text[ranges[0].clone()], "中文");
    }
}
