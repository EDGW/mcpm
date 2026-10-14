use anyhow::{Result, bail};
use serde_json::Value;

use crate::local::mcmod::{VersionConstraint as Constraint, VersionOperator as Operator};

pub(super) fn fabric_json(value: &Value) -> Result<Constraint> {
    json(value, false)
}

pub(super) fn quilt_json(value: &Value) -> Result<Constraint> {
    json(value, true)
}

fn json(value: &Value, bare_version_is_caret: bool) -> Result<Constraint> {
    match value {
        Value::String(expression) => expression_condition(expression, bare_version_is_caret),
        Value::Array(values) => Ok(Constraint::AnyOf(
            values
                .iter()
                .map(|value| json(value, bare_version_is_caret))
                .collect::<Result<_>>()?,
        )),
        Value::Object(values)
            if !values.is_empty() && values.keys().all(|key| key == "all" || key == "any") =>
        {
            let mut conditions = Vec::new();
            for (key, values) in values {
                let values = values.as_array().ok_or_else(|| {
                    anyhow::anyhow!("{key} must contain an array of version conditions")
                })?;
                let conditions_for_key = values
                    .iter()
                    .map(|value| json(value, bare_version_is_caret))
                    .collect::<Result<_>>()?;
                conditions.push(if key == "all" {
                    Constraint::All(conditions_for_key)
                } else {
                    Constraint::AnyOf(conditions_for_key)
                });
            }
            Ok(all(conditions))
        }
        _ => bail!("expected a version string, array, or all/any condition"),
    }
}

fn expression_condition(expression: &str, bare_version_is_caret: bool) -> Result<Constraint> {
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
                conditions.push(predicate(token, bare_version_is_caret)?);
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

fn predicate(token: &str, bare_version_is_caret: bool) -> Result<Constraint> {
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
    Ok(if bare_version_is_caret {
        Constraint::Caret(token.into())
    } else {
        compare(Operator::Equal, token)
    })
}

pub(super) fn maven(expression: &str) -> Result<Constraint> {
    let mut remaining = expression.trim();
    if remaining.is_empty() || remaining == "*" {
        return Ok(Constraint::Any);
    }
    if !remaining.starts_with(['[', '(']) {
        // Maven accepts arbitrary non-range strings as recommended versions.
        // This includes punctuation such as ","; do not reinterpret it as a range.
        return Ok(Constraint::Recommended(remaining.into()));
    }
    let mut alternatives = Vec::new();
    loop {
        let opening = remaining.as_bytes()[0];
        let closing_index = remaining
            .find([']', ')'])
            .ok_or_else(|| anyhow::anyhow!("unclosed Maven version range {expression}"))?;
        let closing = remaining.as_bytes()[closing_index];
        let body = &remaining[1..closing_index];
        let condition = if let Some((minimum, maximum)) = body.split_once(',') {
            // Maven splits at the first comma only. Any additional comma is
            // part of the raw upper-bound version, not a third bound.
            let mut bounds = Vec::new();
            if !minimum.trim().is_empty() {
                bounds.push(compare(
                    if opening == b'[' {
                        Operator::GreaterOrEqual
                    } else {
                        Operator::Greater
                    },
                    minimum.trim(),
                ));
            }
            if !maximum.trim().is_empty() {
                bounds.push(compare(
                    if closing == b']' {
                        Operator::LessOrEqual
                    } else {
                        Operator::Less
                    },
                    maximum.trim(),
                ));
            }
            all(bounds)
        } else {
            if opening != b'[' || closing != b']' || body.trim().is_empty() {
                bail!("invalid exact Maven version range {expression}");
            }
            compare(Operator::Equal, body.trim())
        };
        alternatives.push(condition);
        remaining = remaining[closing_index + 1..].trim();
        if remaining.is_empty() {
            break;
        }
        // Maven permits adjacent ranges and an optional separator, including
        // a trailing comma after the final range.
        remaining = remaining.strip_prefix(',').unwrap_or(remaining).trim();
        if remaining.is_empty() {
            break;
        }
        if !remaining.starts_with(['[', '(']) {
            bail!("expected another Maven range in {expression}");
        }
    }
    Ok(if alternatives.len() == 1 {
        alternatives.into_iter().next().unwrap()
    } else {
        Constraint::AnyOf(alternatives)
    })
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
