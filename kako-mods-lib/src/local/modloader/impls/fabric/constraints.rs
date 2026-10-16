//! Fabric dependency version strings and alternative arrays.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::local::mcmod::{VersionConstraint as Constraint, VersionOperator as Operator};

pub(super) fn fabric_json(value: &Value) -> Result<Constraint> {
    match value {
        Value::String(expression) => expression_condition(expression),
        Value::Array(values) => Ok(Constraint::AnyOf(
            values.iter().map(fabric_json).collect::<Result<_>>()?,
        )),
        _ => bail!("expected a version string or array of version strings"),
    }
}

fn expression_condition(expression: &str) -> Result<Constraint> {
    let alternatives = expression
        .split("||")
        .map(|clause| {
            let tokens: Vec<_> = clause.split_whitespace().collect();
            if tokens.is_empty() {
                bail!("empty dependency version condition");
            }
            if tokens.len() == 3 && tokens[1] == "-" {
                return Ok(Constraint::All(vec![
                    compare(Operator::GreaterOrEqual, tokens[0]),
                    compare(Operator::LessOrEqual, tokens[2]),
                ]));
            }
            let mut conditions = Vec::new();
            let mut index = 0;
            while index < tokens.len() {
                let token = tokens[index];
                let combined;
                let token = if matches!(
                    token,
                    ">=" | "<=" | ">" | "<" | "=" | "==" | "!=" | "~" | "^"
                ) {
                    index += 1;
                    let operand = tokens
                        .get(index)
                        .ok_or_else(|| anyhow::anyhow!("missing version operand after {token}"))?;
                    combined = format!("{token}{operand}");
                    combined.as_str()
                } else {
                    token
                };
                conditions.push(predicate(token)?);
                index += 1;
            }
            Ok(all(conditions))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(if alternatives.len() == 1 {
        alternatives.into_iter().next().unwrap()
    } else {
        Constraint::AnyOf(alternatives)
    })
}

fn predicate(token: &str) -> Result<Constraint> {
    if matches!(token, "*" | "x" | "X") {
        return Ok(Constraint::Any);
    }
    for (prefix, operator) in [
        (">=", Operator::GreaterOrEqual),
        ("<=", Operator::LessOrEqual),
        ("!=", Operator::NotEqual),
        ("==", Operator::Equal),
        (">", Operator::Greater),
        ("<", Operator::Less),
        ("=", Operator::Equal),
    ] {
        if let Some(version) = token.strip_prefix(prefix) {
            if version.is_empty() || version.starts_with(['>', '<', '=', '!', '~', '^']) {
                bail!("missing or invalid version operand in {token}");
            }
            return Ok(compare(operator, version));
        }
    }
    if let Some(version) = token.strip_prefix('^') {
        if version.is_empty() {
            bail!("missing caret version operand");
        }
        return Ok(Constraint::Caret(version.into()));
    }
    if let Some(version) = token.strip_prefix('~') {
        if version.is_empty() {
            bail!("missing tilde version operand");
        }
        return Ok(Constraint::Tilde(version.into()));
    }
    if token
        .split('.')
        .any(|component| matches!(component, "x" | "X" | "*"))
    {
        return Ok(Constraint::Wildcard(token.into()));
    }
    Ok(compare(Operator::Equal, token))
}

fn compare(operator: Operator, version: &str) -> Constraint {
    Constraint::Compare {
        operator,
        version: version.into(),
    }
}

fn all(mut conditions: Vec<Constraint>) -> Constraint {
    match conditions.len() {
        0 => Constraint::Any,
        1 => conditions.pop().unwrap(),
        _ => Constraint::All(conditions),
    }
}
