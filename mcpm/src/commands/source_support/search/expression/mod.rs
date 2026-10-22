//! Provider-independent search expressions compiled into typed remote search criteria.

use std::ops::Range;

use kako_mods_lib::source::SearchQuery;
use thiserror::Error;

use syntax::{Kind, Parser};

mod completion;
mod syntax;

pub(super) use completion::{CompletionContext, CompletionField, completion_context};
pub(super) use syntax::{SyntaxKind, SyntaxSpan};

#[cfg(test)]
mod tests;

/// Unspecified fields inherit CLI/server defaults; an explicit empty list clears them.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Expression {
    keywords: String,
    versions: Option<Vec<String>>,
    loaders: Option<Vec<String>>,
}

impl Expression {
    pub(super) fn parse(source: &str) -> Result<Self, ParseError> {
        Self::parse_with(&mut Parser::new(source))
    }

    /// Keep token spans even for unfinished drafts; validation never sends a request.
    pub(super) fn inspect(source: &str) -> (Result<Self, ParseError>, Vec<SyntaxSpan>) {
        let mut parser = Parser::new(source);
        let result = Self::parse_with(&mut parser);
        // Semantic errors may stop before the end. Finish lexing for editor colors.
        while !matches!(parser.next(), Ok(None)) {}
        (result, parser.syntax)
    }

    fn parse_with(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let mut words = Vec::new();
        let mut expression = Self {
            keywords: String::new(),
            versions: None,
            loaders: None,
        };
        let mut fields_started = false;
        while let Some(token) = parser.next()? {
            match token.kind {
                Kind::Word(value) if !fields_started => words.push(value),
                Kind::Field(field) => {
                    fields_started = true;
                    let (target, loader) = match field.as_str() {
                        "version" => (&mut expression.versions, false),
                        "loader" => (&mut expression.loaders, true),
                        _ => {
                            return Err(
                                parser.error(token.start, "unknown field; use @version or @loader")
                            );
                        }
                    };
                    if target.is_some() {
                        return Err(parser
                            .error(token.start, "duplicate field; put alternatives in (A | B)"));
                    }
                    *target = Some(parser.values(loader)?);
                }
                _ => {
                    return Err(parser.error(
                        token.start,
                        "expected @version or @loader; keywords must precede fields",
                    ));
                }
            }
        }
        expression.keywords = words.join(" ");
        Ok(expression)
    }

    pub(super) fn apply(self, defaults: &SearchQuery) -> SearchQuery {
        SearchQuery {
            query: self.keywords,
            game_versions: self
                .versions
                .unwrap_or_else(|| defaults.game_versions.clone()),
            loaders: self.loaders.unwrap_or_else(|| defaults.loaders.clone()),
            offset: 0,
            ..defaults.clone()
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("search expression at column {column}: {message}")]
pub(super) struct ParseError {
    pub(super) column: usize,
    pub(super) range: Range<usize>,
    pub(super) message: &'static str,
}

pub(super) const HELP: &str = "Search expression: keywords followed by @version VALUE and/or @loader VALUE.\nDifferent fields use AND; (A | B) matches either value within a field.\nUse * as a field value to remove its filter, including server defaults.\nFields override CLI options, which override server defaults.\nVersions and loaders match exactly; loader names ignore case.\nQuote a keyword phrase inside the expression to include syntax literally.\nEscape syntax characters with a backslash; quote backslashes as \\\\.\nUnknown/repeated fields are errors. Ranges, regex and general boolean operators are unsupported.\nCurseForge allows at most 4 versions and 5 loaders per search.\nExample: create @version (1.20.1 | 1.21.1) @loader (forge | neoforge)";
