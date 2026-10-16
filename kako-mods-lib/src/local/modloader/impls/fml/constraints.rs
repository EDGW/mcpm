//! Maven version-range syntax used by FML dependency declarations.

use anyhow::{Result, bail};

use crate::local::mcmod::{VersionConstraint as Constraint, VersionOperator as Operator};

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
