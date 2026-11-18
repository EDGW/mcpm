//! Restricted token extraction and operator detection for static launch evidence.

use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Syntax {
    Shell,
    Batch,
}

#[derive(Debug, thiserror::Error)]
#[error("unresolved {shell} variable {name}")]
pub(super) struct UnresolvedVariable {
    pub name: String,
    shell: &'static str,
}

/// A deliberately restricted lexer. Expansion never becomes executable syntax.
pub(super) fn split(
    input: &str,
    syntax: Syntax,
    vars: &HashMap<String, String>,
    script: &Path,
) -> Result<Vec<String>> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut quote = None;
    let mut active = false;
    let mut token = String::new();
    let mut tokens = Vec::new();
    while i < chars.len() {
        let ch = chars[i];
        if quote.is_none() && ch.is_whitespace() {
            if active {
                tokens.push(std::mem::take(&mut token));
                active = false;
            }
            i += 1;
            continue;
        }
        if quote.is_none() && ch == '#' && syntax != Syntax::Batch && !active {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '"' || (ch == '\'' && syntax != Syntax::Batch) {
            if quote == Some(ch) {
                quote = None;
                i += 1;
                continue;
            }
            if quote.is_none() {
                quote = Some(ch);
                active = true;
                i += 1;
                continue;
            }
        }
        let literal = quote == Some('\'');
        if syntax == Syntax::Shell && !literal && ch == '$' {
            let (value, consumed) = if chars.get(i + 1) == Some(&'0') {
                (script.to_string_lossy().into_owned(), 2)
            } else {
                let start = i + 1;
                let braced = chars.get(start) == Some(&'{');
                let name_start = start + usize::from(braced);
                let mut end = name_start;
                while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                {
                    end += 1;
                }
                ensure!(end > name_start, "unsupported shell expansion");
                if braced {
                    ensure!(
                        chars.get(end) == Some(&'}'),
                        "unsupported shell variable expression"
                    );
                }
                let name: String = chars[name_start..end].iter().collect();
                let value = vars.get(&name).ok_or_else(|| UnresolvedVariable {
                    name,
                    shell: "shell",
                })?;
                (value.clone(), end + usize::from(braced) - i)
            };
            ensure!(
                quote.is_some()
                    || (!value.is_empty()
                        && !value.contains(|c: char| c.is_whitespace() || "*?[".contains(c))),
                "unquoted expansion requires shell word splitting or globbing"
            );
            token.push_str(&value);
            active = true;
            i += consumed;
            continue;
        }
        if syntax == Syntax::Batch && ch == '%' {
            // Inspection supplies no script arguments. Preserve token/quote state:
            // bare %* disappears, while quoted or embedded %* keeps its surroundings.
            if chars.get(i + 1) == Some(&'*') {
                i += 2;
                continue;
            }
            let tail: String = chars[i..].iter().collect();
            let (value, consumed) = if tail.to_ascii_lowercase().starts_with("%~dp0") {
                (format!("{}/", script.parent().unwrap().display()), 5)
            } else {
                let end = (i + 1..chars.len())
                    .find(|&n| chars[n] == '%')
                    .ok_or_else(|| anyhow::anyhow!("unsupported batch expansion"))?;
                let name: String = chars[i + 1..end]
                    .iter()
                    .collect::<String>()
                    .to_ascii_uppercase();
                let value = vars.get(&name).ok_or_else(|| UnresolvedVariable {
                    name,
                    shell: "batch",
                })?;
                (value.clone(), end - i + 1)
            };
            ensure!(
                !value.contains(|c: char| "&|<>^\"%".contains(c))
                    && (quote.is_some() || !value.contains(char::is_whitespace)),
                "unsafe unquoted batch expansion"
            );
            token.push_str(&value);
            active = true;
            i += consumed;
            continue;
        }
        if syntax == Syntax::Batch && ch == '!' {
            bail!("delayed batch expansion is unsupported");
        }
        let escape = (syntax == Syntax::Shell && !literal && ch == '\\')
            || (syntax == Syntax::Batch && quote.is_none() && ch == '^');
        if escape {
            let next = *chars
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("unfinished escape"))?;
            if syntax == Syntax::Shell && quote == Some('"') && !"$`\"\\".contains(next) {
                token.push(ch);
                i += 1;
                active = true;
                continue;
            }
            token.push(next);
            active = true;
            i += 2;
            continue;
        }
        if syntax == Syntax::Shell && !literal && ch == '`' {
            bail!("command substitution is unsupported");
        }
        if quote.is_none() {
            ensure!(
                !";&|<>()".contains(ch),
                "shell operators and control flow are unsupported"
            );
            if syntax == Syntax::Shell {
                ensure!(
                    !"*?[~".contains(ch),
                    "globbing and tilde expansion are unsupported"
                );
            }
        }
        token.push(ch);
        active = true;
        i += 1;
    }
    ensure!(quote.is_none(), "unterminated quote");
    if active {
        tokens.push(token);
    }
    Ok(tokens)
}

/// Lexical words retain their original quoting; operators never come from expansions.
#[derive(Debug)]
pub(super) enum Part<'a> {
    Word(&'a str),
    Operator(&'a str),
}

impl Part<'_> {
    pub(super) fn is_redirection(&self) -> bool {
        matches!(
            self,
            Self::Operator("<" | ">" | "<<" | ">>" | "<&" | ">&" | "&>")
        )
    }
}

pub(super) struct Inspection<'a> {
    pub parts: Vec<Part<'a>>,
    pub unsupported: bool,
}

/// Inspect structure without expanding variables or interpreting launch arguments.
pub(super) fn inspect(input: &str, syntax: Syntax) -> Inspection<'_> {
    let mut parts = Vec::new();
    let mut unsupported = false;
    let mut start = None;
    let mut quote = None;
    let mut index = 0;
    let bytes = input.as_bytes();
    while index < bytes.len() {
        let ch = bytes[index];
        if (syntax == Syntax::Shell && quote != Some(b'\'') && ch == b'\\')
            || (syntax == Syntax::Batch && quote.is_none() && ch == b'^')
        {
            start.get_or_insert(index);
            index = (index + 2).min(bytes.len());
            continue;
        }
        if syntax == Syntax::Shell
            && quote != Some(b'\'')
            && (ch == b'`' || (ch == b'$' && bytes.get(index + 1) == Some(&b'(')))
        {
            unsupported = true;
            start.get_or_insert(index);
            index = substitution_end(bytes, index);
            continue;
        }
        if ch == b'"' || (syntax == Syntax::Shell && ch == b'\'') {
            start.get_or_insert(index);
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            index += 1;
            continue;
        }
        if quote.is_none() {
            if syntax == Syntax::Shell && ch == b'#' && start.is_none() {
                break;
            }
            if ch.is_ascii_whitespace() || b";&|<>()".contains(&ch) {
                if let Some(begin) = start.take()
                    && !(matches!(ch, b'<' | b'>')
                        && input[begin..index].bytes().all(|ch| ch.is_ascii_digit()))
                {
                    parts.push(Part::Word(&input[begin..index]));
                }
                if !ch.is_ascii_whitespace() {
                    unsupported = true;
                    let begin = index;
                    if (b"&|<>;".contains(&ch) && bytes.get(index + 1) == Some(&ch))
                        || (matches!(ch, b'<' | b'>') && bytes.get(index + 1) == Some(&b'&'))
                        || (syntax == Syntax::Shell
                            && ch == b'&'
                            && bytes.get(index + 1) == Some(&b'>'))
                    {
                        index += 1;
                    }
                    parts.push(Part::Operator(&input[begin..=index]));
                }
                index += 1;
                continue;
            }
        }
        start.get_or_insert(index);
        index += 1;
    }
    if let Some(begin) = start {
        parts.push(Part::Word(&input[begin..index]));
    }
    Inspection { parts, unsupported }
}

/// Skip a substitution's contents: its commands cannot modify the parent shell state.
fn substitution_end(bytes: &[u8], start: usize) -> usize {
    let backtick = bytes[start] == b'`';
    let mut depth = 1;
    let mut quote = None;
    let mut index = start + if backtick { 1 } else { 2 };
    while index < bytes.len() {
        let ch = bytes[index];
        if ch == b'\\' && quote != Some(b'\'') {
            index = (index + 2).min(bytes.len());
            continue;
        }
        if backtick && ch == b'`' {
            return index + 1;
        }
        if ch == b'"' || ch == b'\'' {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        } else if !backtick && quote.is_none() {
            if ch == b'(' {
                depth += 1;
            }
            if ch == b')' {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
        }
        index += 1;
    }
    index
}
