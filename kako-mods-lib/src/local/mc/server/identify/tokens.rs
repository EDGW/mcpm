use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Syntax {
    Shell,
    Batch,
    Java,
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
            let tail: String = chars[i..].iter().collect();
            let (value, consumed) = if tail.starts_with("$(dirname \"$0\")") {
                (
                    script.parent().unwrap().to_string_lossy().into_owned(),
                    "$(dirname \"$0\")".chars().count(),
                )
            } else if chars.get(i + 1) == Some(&'0') {
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
                let value = vars
                    .get(&name)
                    .ok_or_else(|| anyhow::anyhow!("unresolved shell variable {name}"))?
                    .clone();
                (value, end + usize::from(braced) - i)
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
                (
                    vars.get(&name)
                        .ok_or_else(|| anyhow::anyhow!("unresolved batch variable {name}"))?
                        .clone(),
                    end - i + 1,
                )
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
            || (syntax == Syntax::Batch && quote.is_none() && ch == '^')
            || (syntax == Syntax::Java && quote.is_some() && ch == '\\');
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
            if syntax == Syntax::Java {
                token.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'f' => '\u{c}',
                    '\\' | '\'' | '"' => next,
                    _ => bail!("unsupported Java argument escape"),
                });
            } else {
                token.push(next);
            }
            active = true;
            i += 2;
            continue;
        }
        if syntax == Syntax::Shell && !literal && ch == '`' {
            bail!("command substitution is unsupported");
        }
        if quote.is_none() && syntax != Syntax::Java {
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
