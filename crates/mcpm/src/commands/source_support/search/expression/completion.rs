//! Locate a field value at the cursor using the expression lexer, preserving surrounding syntax.

use std::ops::Range;

use super::syntax::{Kind, Parser};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::commands::source_support::search) enum CompletionField {
    Loader,
    Version,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::commands::source_support::search) struct CompletionContext {
    pub range: Range<usize>,
    pub prefix: String,
    pub field: CompletionField,
    pub separator: bool,
}

pub(in crate::commands::source_support::search) fn completion_context(
    source: &str,
    cursor: usize,
) -> Option<CompletionContext> {
    source.get(..cursor)?;
    let mut parser = Parser::new(source);
    // Field, whether inside an OR group, whether a value is expected, previous token end.
    let mut active = None;
    while let Some(token) = parser.next().ok()? {
        let end = parser.position();
        if let Some((field, _, true, after)) = active
            && cursor >= after
            && cursor < token.start
            && !matches!(token.kind, Kind::Open)
        {
            let range = if matches!(token.kind, Kind::Word(_)) {
                token.start..end
            } else {
                cursor..cursor
            };
            return literal_context(source, cursor, range, field);
        }
        match token.kind {
            Kind::Field(name) => {
                active = match name.as_str() {
                    "loader" => Some((CompletionField::Loader, false, true, end)),
                    "version" => Some((CompletionField::Version, false, true, end)),
                    _ => None,
                };
            }
            Kind::Word(_) => {
                if let Some((field, group, true, _)) = active {
                    if (token.start..=end).contains(&cursor) {
                        return literal_context(source, cursor, token.start..end, field);
                    }
                    active = group.then_some((field, group, false, end));
                } else {
                    active = None;
                }
            }
            Kind::Open => {
                active = active.and_then(|(field, group, expected, _)| {
                    (!group && expected).then_some((field, true, true, end))
                })
            }
            Kind::Or => {
                active = active.and_then(|(field, group, expected, _)| {
                    (group && !expected).then_some((field, true, true, end))
                })
            }
            Kind::Close => {
                if let Some((field, _, true, after)) = active
                    && (after..=token.start).contains(&cursor)
                {
                    return literal_context(source, cursor, cursor..cursor, field);
                }
                active = None;
            }
        }
    }
    let (field, _, true, after) = active? else {
        return None;
    };
    (cursor >= after)
        .then(|| literal_context(source, cursor, cursor..cursor, field))
        .flatten()
}

fn literal_context(
    source: &str,
    cursor: usize,
    range: Range<usize>,
    field: CompletionField,
) -> Option<CompletionContext> {
    let raw = &source[range.clone()];
    // Quotes, escapes and * are literal syntax, never completion placeholders.
    if !raw
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-' | b'_' | b'+'))
    {
        return None;
    }
    let prefix_end = cursor.clamp(range.start, range.end);
    let separator = range.is_empty()
        && source[..range.start]
            .chars()
            .next_back()
            .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, '(' | '|'));
    Some(CompletionContext {
        prefix: source[range.start..prefix_end].into(),
        range,
        field,
        separator,
    })
}
