//! Character-aware tokenization and bounded OR groups for remote search fields.

use super::ParseError;

pub(super) struct Parser<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    pub(super) fn error(&self, start: usize, message: &'static str) -> ParseError {
        ParseError {
            column: self.source[..start].chars().count() + 1,
            message,
        }
    }

    pub(super) fn next(&mut self) -> Result<Option<Token>, ParseError> {
        while self.peek().is_some_and(char::is_whitespace) {
            self.take();
        }
        let start = self.position;
        let Some(ch) = self.take() else {
            return Ok(None);
        };
        let kind = match ch {
            '(' => Kind::Open,
            ')' => Kind::Close,
            '|' => Kind::Or,
            '&' => {
                return Err(self.error(start, "fields are combined with AND automatically; omit &"));
            }
            '"' | '\'' => {
                let mut value = String::new();
                loop {
                    match self.take() {
                        None => return Err(self.error(start, "unclosed quote")),
                        Some(end) if end == ch => break,
                        Some('\\') => value.push(self.escaped(start)?),
                        Some(ch) => value.push(ch),
                    }
                }
                if self
                    .peek()
                    .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, '(' | ')' | '|'))
                {
                    return Err(
                        self.error(self.position, "expected a separator after closing quote")
                    );
                }
                Kind::Word(value)
            }
            _ => {
                let field = ch == '@';
                let mut value = String::new();
                if !field {
                    value.push(if ch == '\\' { self.escaped(start)? } else { ch });
                }
                while let Some(ch) = self.peek() {
                    if ch.is_whitespace() || matches!(ch, '(' | ')' | '|' | '&') {
                        break;
                    }
                    self.take();
                    value.push(if ch == '\\' { self.escaped(start)? } else { ch });
                }
                if field {
                    Kind::Field(value)
                } else {
                    Kind::Word(value)
                }
            }
        };
        Ok(Some(Token { start, kind }))
    }

    pub(super) fn values(&mut self, loader: bool) -> Result<Vec<String>, ParseError> {
        let token = self
            .next()?
            .ok_or_else(|| self.error(self.position, "missing field value"))?;
        if let Kind::Word(value) = &token.kind
            && value == "*"
        {
            return Ok(Vec::new());
        }
        if token.kind != Kind::Open {
            return Ok(vec![self.value(token, loader)?]);
        }
        let mut values = Vec::new();
        loop {
            let token = self
                .next()?
                .ok_or_else(|| self.error(self.position, "unclosed group; expected a value"))?;
            let value = self.value(token, loader)?;
            if !values.contains(&value) {
                values.push(value);
            }
            match self.next()? {
                Some(Token {
                    kind: Kind::Close, ..
                }) => return Ok(values),
                Some(Token { kind: Kind::Or, .. }) => {}
                Some(token) => return Err(self.error(token.start, "expected | or )")),
                None => return Err(self.error(self.position, "unclosed group; expected | or )")),
            }
        }
    }

    fn value(&self, token: Token, loader: bool) -> Result<String, ParseError> {
        let Kind::Word(value) = token.kind else {
            return Err(self.error(
                token.start,
                "expected a literal field value; nested groups are unsupported",
            ));
        };
        if value.is_empty()
            || !value
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-' | b'_' | b'+'))
        {
            return Err(self.error(
                token.start,
                "expected an exact version or loader; * must be used alone",
            ));
        }
        Ok(if loader {
            value.to_ascii_lowercase()
        } else {
            value
        })
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn take(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.position += ch.len_utf8();
        Some(ch)
    }

    fn escaped(&mut self, start: usize) -> Result<char, ParseError> {
        self.take()
            .ok_or_else(|| self.error(start, "trailing escape"))
    }
}

pub(super) struct Token {
    pub(super) start: usize,
    pub(super) kind: Kind,
}

#[derive(PartialEq, Eq)]
pub(super) enum Kind {
    Word(String),
    Field(String),
    Open,
    Close,
    Or,
}
