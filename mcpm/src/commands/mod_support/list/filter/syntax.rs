//! Filter tokenization, source spans and boolean precedence with precise diagnostics.

use std::ops::Range;

use regex::RegexBuilder;
use thiserror::Error;

use super::{super::columns::Column, Expr, Matcher};

pub(super) fn parse(source: &str) -> (Result<Option<Expr>, FilterError>, Vec<SyntaxSpan>) {
    let mut lexer = Lexer {
        source,
        offset: 0,
        syntax: Vec::new(),
    };
    let tokens = lexer.tokens();
    let expression = tokens.and_then(|tokens| {
        let mut parser = Parser {
            source,
            tokens,
            offset: 0,
        };
        if parser.tokens.is_empty() {
            return Ok(None);
        }
        let expr = parser.or(0, None)?;
        if parser.peek().is_some() {
            return Err(parser.error(ErrorKind::UnexpectedToken));
        }
        Ok(Some(expr))
    });
    (expression, lexer.syntax)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::commands::mod_support::list) enum SyntaxKind {
    Field,
    Value,
    Operator,
    Delimiter,
    Escape,
}

#[derive(Clone, Debug)]
pub(in crate::commands::mod_support::list) struct SyntaxSpan {
    pub range: Range<usize>,
    pub kind: SyntaxKind,
}

#[derive(Debug)]
struct Token {
    kind: Kind,
    range: Range<usize>,
}

#[derive(Debug)]
enum Kind {
    Value { text: String, quoted: bool },
    Field(Option<Column>),
    And,
    Or,
    Equal,
    Regex,
    Left,
    Right,
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    offset: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.offset)
    }

    fn or(&mut self, depth: usize, scope: Option<Column>) -> Result<Expr, FilterError> {
        let mut terms = vec![self.and(depth, scope)?];
        while matches!(self.peek().map(|token| &token.kind), Some(Kind::Or)) {
            self.offset += 1;
            terms.push(self.and(depth, scope)?);
        }
        Ok(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::Or(terms)
        })
    }

    fn and(&mut self, depth: usize, scope: Option<Column>) -> Result<Expr, FilterError> {
        let mut terms = vec![self.term(depth, scope)?];
        loop {
            match self.peek().map(|token| &token.kind) {
                Some(Kind::And) => {
                    self.offset += 1;
                }
                Some(
                    Kind::Field(_) | Kind::Value { .. } | Kind::Equal | Kind::Regex | Kind::Left,
                ) => {
                    let previous = &self.tokens[self.offset - 1];
                    let next = &self.tokens[self.offset];
                    if previous.range.end == next.range.start {
                        return Err(self.error(ErrorKind::MissingAnd));
                    }
                }
                _ => break,
            }
            terms.push(self.term(depth, scope)?);
        }
        Ok(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::And(terms)
        })
    }

    fn term(&mut self, depth: usize, scope: Option<Column>) -> Result<Expr, FilterError> {
        let field = match self.peek().map(|token| &token.kind) {
            Some(Kind::Field(Some(field))) => {
                let field = *field;
                self.offset += 1;
                Some(field)
            }
            Some(Kind::Field(None)) => {
                let range = self.peek().unwrap().range.clone();
                return Err(self.error(ErrorKind::UnknownField(self.source[range].into())));
            }
            _ => scope,
        };
        if matches!(self.peek().map(|token| &token.kind), Some(Kind::Left)) {
            let opening = self.peek().unwrap().range.clone();
            if depth >= 64 {
                return Err(self.error(ErrorKind::TooDeep));
            }
            self.offset += 1;
            let expr = self.or(depth + 1, field)?;
            if !matches!(self.peek().map(|token| &token.kind), Some(Kind::Right)) {
                return Err(FilterError::new(
                    self.source,
                    opening,
                    ErrorKind::UnclosedGroup,
                ));
            }
            self.offset += 1;
            return Ok(expr);
        }
        let mode = match self.peek().map(|token| &token.kind) {
            Some(Kind::Equal) => {
                self.offset += 1;
                1
            }
            Some(Kind::Regex) => {
                self.offset += 1;
                2
            }
            _ => 0,
        };
        let Some(Token {
            kind: Kind::Value { text, quoted },
            range,
        }) = self.peek()
        else {
            return Err(self.error(ErrorKind::MissingValue));
        };
        let matcher = match mode {
            1 => Matcher::Exact(text.to_lowercase()),
            2 => {
                if !quoted {
                    return Err(self.error(ErrorKind::UnquotedRegex));
                }
                let regex = RegexBuilder::new(text)
                    .case_insensitive(true)
                    .build()
                    .map_err(|error| {
                        FilterError::new(
                            self.source,
                            range.clone(),
                            ErrorKind::InvalidRegex(
                                error
                                    .to_string()
                                    .lines()
                                    .last()
                                    .unwrap_or("invalid regex")
                                    .into(),
                            ),
                        )
                    })?;
                Matcher::Regex(regex)
            }
            _ => Matcher::Contains(text.to_lowercase()),
        };
        self.offset += 1;
        Ok(Expr::Predicate { field, matcher })
    }

    fn error(&self, kind: ErrorKind) -> FilterError {
        FilterError::new(
            self.source,
            self.peek()
                .map_or(self.source.len()..self.source.len(), |token| {
                    token.range.clone()
                }),
            kind,
        )
    }
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    syntax: Vec<SyntaxSpan>,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn take(&mut self) -> char {
        let ch = self.peek().expect("lexer has a character");
        self.offset += ch.len_utf8();
        ch
    }

    fn span(&mut self, range: Range<usize>, kind: SyntaxKind) {
        if !range.is_empty() {
            self.syntax.push(SyntaxSpan { range, kind });
        }
    }

    fn tokens(&mut self) -> Result<Vec<Token>, FilterError> {
        let mut tokens = Vec::new();
        let mut error = None;
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.take();
                continue;
            }
            let start = self.offset;
            let kind = match ch {
                '&' | '|' | '=' | '~' | '(' | ')' => {
                    self.take();
                    self.span(
                        start..self.offset,
                        if matches!(ch, '(' | ')') {
                            SyntaxKind::Delimiter
                        } else {
                            SyntaxKind::Operator
                        },
                    );
                    match ch {
                        '&' => Kind::And,
                        '|' => Kind::Or,
                        '=' => Kind::Equal,
                        '~' => Kind::Regex,
                        '(' => Kind::Left,
                        ')' => Kind::Right,
                        _ => unreachable!(),
                    }
                }
                '@' => {
                    self.take();
                    while self
                        .peek()
                        .is_some_and(|ch| !boundary(ch) && !matches!(ch, '=' | '~'))
                    {
                        self.take();
                    }
                    let field = match self.source[start..self.offset]
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "@name" => Some(Column::Name),
                        "@modid" => Some(Column::Modid),
                        "@file" => Some(Column::Filename),
                        "@loaders" => Some(Column::Loaders),
                        _ => None,
                    };
                    self.span(start..self.offset, SyntaxKind::Field);
                    Kind::Field(field)
                }
                _ => {
                    let (kind, failure) = self.value();
                    if error.is_none() {
                        error = failure;
                    }
                    kind
                }
            };
            tokens.push(Token {
                kind,
                range: start..self.offset,
            });
        }
        error.map_or(Ok(tokens), Err)
    }

    fn value(&mut self) -> (Kind, Option<FilterError>) {
        let start = self.offset;
        let quote = self.peek().filter(|ch| matches!(ch, '\'' | '"'));
        if quote.is_some() {
            self.take();
            self.span(start..self.offset, SyntaxKind::Delimiter);
        }
        let mut text = String::new();
        let mut plain = self.offset;
        while let Some(ch) = self.peek() {
            if Some(ch) == quote {
                self.span(plain..self.offset, SyntaxKind::Value);
                let end = self.offset;
                self.take();
                self.span(end..self.offset, SyntaxKind::Delimiter);
                return (Kind::Value { text, quoted: true }, None);
            }
            if quote.is_none() && boundary(ch) {
                break;
            }
            let at = self.offset;
            self.take();
            if ch == '\\'
                && let Some(next) = self.peek()
            {
                let escaped = if let Some(quote) = quote {
                    next == quote || next == '\\'
                } else {
                    boundary(next) || matches!(next, '@' | '=' | '~' | '\\')
                };
                if escaped {
                    self.span(plain..at, SyntaxKind::Value);
                    text.push(self.take());
                    self.span(at..self.offset, SyntaxKind::Escape);
                    plain = self.offset;
                    continue;
                }
            }
            text.push(ch);
        }
        self.span(plain..self.offset, SyntaxKind::Value);
        let error = quote.map(|quote| {
            FilterError::new(
                self.source,
                start..self.offset,
                ErrorKind::UnclosedQuote(quote),
            )
        });
        (
            Kind::Value {
                text,
                quoted: quote.is_some(),
            },
            error,
        )
    }
}

fn boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '&' | '|' | '(' | ')' | '\'' | '"')
}

#[derive(Clone, Debug, Error)]
#[error("{kind} at column {column}")]
pub(in crate::commands) struct FilterError {
    pub(in crate::commands::mod_support::list) range: Range<usize>,
    column: usize,
    kind: ErrorKind,
}

impl FilterError {
    fn new(source: &str, range: Range<usize>, kind: ErrorKind) -> Self {
        Self {
            column: source[..range.start].chars().count() + 1,
            range,
            kind,
        }
    }
}

#[derive(Clone, Debug, Error)]
enum ErrorKind {
    #[error("expected a matching value")]
    MissingValue,
    #[error("expected whitespace or '&' between conditions")]
    MissingAnd,
    #[error("unexpected token")]
    UnexpectedToken,
    #[error("unknown field {0}; use @name, @modid, @file or @loaders")]
    UnknownField(String),
    #[error("unclosed {0} quote")]
    UnclosedQuote(char),
    #[error("unclosed parenthesis")]
    UnclosedGroup,
    #[error("regex patterns must be quoted")]
    UnquotedRegex,
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
    #[error("parentheses are nested too deeply (maximum 64)")]
    TooDeep,
}
