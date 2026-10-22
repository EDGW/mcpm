//! Typed list filters shared by CLI validation, row matching and TUI highlighting.

use std::{fmt, ops::Range, str::FromStr};

use super::{columns::Column, rows::Row};

mod matcher;
mod syntax;

use matcher::Matcher;
pub(in crate::commands) use syntax::FilterError;
pub(super) use syntax::{SyntaxKind, SyntaxSpan};

/// A parsed expression retains its source and tokens, including incomplete TUI input.
/// `FromStr` rejects errors at the CLI boundary; `parse` also supports live editing.
#[derive(Clone, Debug)]
pub(in crate::commands) struct Filter {
    source: String,
    expression: Result<Option<Expr>, FilterError>,
    pub(super) syntax: Vec<SyntaxSpan>,
}

impl Filter {
    pub(super) fn parse(source: &str) -> Self {
        let (expression, syntax) = syntax::parse(source);
        Self {
            source: source.into(),
            expression,
            syntax,
        }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.source
    }

    pub(super) fn error(&self) -> Option<&FilterError> {
        self.expression.as_ref().err()
    }

    pub(super) fn invalid(&self) -> bool {
        self.error().is_some()
    }

    pub(super) fn matches(&self, row: &Row) -> bool {
        match &self.expression {
            Ok(Some(expr)) => expr.matches(&Document::new(row)),
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// Only successful boolean branches contribute hits; ancestor context and
    /// failed AND branches must not highlight unrelated text.
    pub(super) fn ranges(&self, row: &Row, column: Column) -> Vec<Range<usize>> {
        if column == Column::Index {
            return Vec::new();
        }
        let Ok(Some(expr)) = &self.expression else {
            return Vec::new();
        };
        let doc = Document::new(row);
        let mut ranges = Vec::new();
        expr.ranges(&doc, column, &mut ranges);
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
}

impl Default for Filter {
    fn default() -> Self {
        Self::parse("")
    }
}

impl FromStr for Filter {
    type Err = FilterError;
    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let filter = Self::parse(source);
        if let Some(error) = filter.error() {
            return Err(error.clone());
        }
        Ok(filter)
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}

const FIELDS: [Column; 4] = [
    Column::Name,
    Column::Modid,
    Column::Filename,
    Column::Loaders,
];

#[derive(Clone, Debug)]
enum Expr {
    Predicate {
        field: Option<Column>,
        matcher: Matcher,
    },
    And(Vec<Self>),
    Or(Vec<Self>),
}

impl Expr {
    fn matches(&self, doc: &Document) -> bool {
        match self {
            Self::Predicate { field, matcher } => FIELDS
                .into_iter()
                .filter(|column| field.is_none_or(|field| field == *column))
                .any(|column| {
                    doc.values[column.position()]
                        .iter()
                        .any(|range| matcher.matches(&doc.text[column.position()][range.clone()]))
                }),
            Self::And(terms) => terms.iter().all(|term| term.matches(doc)),
            Self::Or(terms) => terms.iter().any(|term| term.matches(doc)),
        }
    }

    fn ranges(&self, doc: &Document, column: Column, ranges: &mut Vec<Range<usize>>) {
        if !self.matches(doc) {
            return;
        }
        match self {
            Self::Predicate { field, matcher } if field.is_none_or(|field| field == column) => {
                for value in &doc.values[column.position()] {
                    ranges.extend(
                        matcher
                            .ranges(&doc.text[column.position()][value.clone()])
                            .into_iter()
                            .map(|range| range.start + value.start..range.end + value.start),
                    );
                }
            }
            Self::And(terms) | Self::Or(terms) => {
                for term in terms {
                    term.ranges(doc, column, ranges);
                }
            }
            _ => {}
        }
    }
}

/// Semantic value ranges come from the row producer, never from parsing display decoration.
#[derive(Debug)]
struct Document {
    text: [String; 5],
    values: [Vec<Range<usize>>; 5],
}

impl Document {
    fn new(row: &Row) -> Self {
        let fields = Column::ALL.map(|column| row.searchable(column));
        Self {
            text: fields.each_ref().map(|(text, _)| text.clone()),
            values: fields.map(|(_, values)| values),
        }
    }
}

pub(super) const HELP: &str = r#"Filter rows in TUI, Console and Raw; TUI starts with this editable expression.

Type: Filter

Filter syntax:
  text / 'text' / "text"   Case-insensitive contains matching.
  =text / ="text"         Case-insensitive whole-value matching.
  ~"pattern" / ~'pattern' Rust regex; case-insensitive by default.
  @name VALUE             Match Name only.
  @modid VALUE            Match Mod ID only.
  @file VALUE             Match the displayed file/archive path only.
  @loaders VALUE          Match individual loader names, without brackets.
  @loaders (Forge | Fabric) Apply a field to an entire group.
  A B / A & B             AND (explicit & or implicit whitespace).
  A | B                   OR; AND binds more tightly than OR.
  (A | B) & C             Parentheses override precedence.

A field applies to the next value or parenthesized group. Unqualified values
inside that group inherit the field; an explicit inner field overrides it.
@loaders Forge | Fabric scopes only Forge; use parentheses to scope both.
Unscoped matchers search Name, Mod ID, File and Loaders, including hidden
descendants; Index is excluded. Empty filters match everything. ="" matches
an empty value. Regex searches substrings; use ^ and $ for whole values and
(?-i) for case-sensitive matching. Rust regex does not support look-around
or backreferences. Zero-width regex matches filter rows but highlight no text.

Outside quotes, escape syntax characters with a backslash:
  \@ \( \) \& \| \= \~ \" \' \\ and backslash followed by a space.
Inside quotes, escape the matching quote or backslash; all other backslashes
stay literal, so ~'\d+' works. Quoted @, &, | and parentheses are literal.
Quote phrases containing spaces. Unknown fields, missing operands, invalid
regex and unclosed quotes/parentheses are errors, reported with a column.

Examples (quote the entire expression for your shell):
  -F "@modid 'Fabric'"
  -F "(@name sodium | @name lithium) & @loaders =fabric"
  -F "@name ='Fabric API' | @file ~'(?-i)API.*[.]jar$'"
  -F "@file user\@host.jar"

Input selection (--file/--modid/--name/--all on server mods list) happens
before filtering and is independent of this expression. TUI keeps ancestor
rows as context. Console/Raw print matching declarations; without --expanded,
only the first matching declaration per input file is printed."#;

#[cfg(test)]
mod tests;
