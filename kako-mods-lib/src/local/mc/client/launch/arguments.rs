use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::UnknownVariablePolicy;
use crate::local::mc::client::{LibraryRule, LibraryRuleError, RuleEnvironment};

#[derive(Debug, Error)]
pub enum ArgumentError {
    #[error("invalid {kind} argument at index {index}")]
    Invalid {
        kind: &'static str,
        index: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Rule(#[from] LibraryRuleError),
    #[error("legacy minecraftArguments contains unmatched quotes or escapes")]
    LegacySyntax,
    #[error("unknown launch placeholder ${{{name}}}")]
    UnknownVariable { name: String },
    #[error("unterminated launch placeholder")]
    UnterminatedVariable,
    #[error("custom variable {name:?} attempts to override a reserved launch value")]
    ReservedVariable { name: String },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Argument {
    Literal(String),
    Conditional {
        rules: Vec<LibraryRule>,
        value: ArgumentValue,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ArgumentValue {
    One(String),
    Many(Vec<String>),
}

pub(super) fn evaluate(
    values: Vec<&Value>,
    rules: &RuleEnvironment,
    kind: &'static str,
) -> Result<Vec<OsString>, ArgumentError> {
    let mut output = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        match serde_json::from_value(value.clone()).map_err(|source| ArgumentError::Invalid {
            kind,
            index,
            source,
        })? {
            Argument::Literal(value) => output.push(value.into()),
            Argument::Conditional {
                rules: conditions,
                value,
            } if rules.allows(&conditions)? => match value {
                ArgumentValue::One(value) => output.push(value.into()),
                ArgumentValue::Many(values) => {
                    output.extend(values.into_iter().map(OsString::from))
                }
            },
            Argument::Conditional { .. } => {}
        }
    }
    Ok(output)
}

pub(super) fn legacy(value: &str) -> Result<Vec<OsString>, ArgumentError> {
    // Tokenize before substitution: spaces inside player names or paths remain one argument.
    shlex::split(value)
        .ok_or(ArgumentError::LegacySyntax)
        .map(|values| values.into_iter().map(OsString::from).collect())
}

pub(super) fn expand(
    value: &OsStr,
    variables: &BTreeMap<String, OsString>,
    policy: UnknownVariablePolicy,
) -> Result<OsString, ArgumentError> {
    // Literal caller arguments can contain non-UTF-8 OS paths. Only UTF-8 strings
    // have manifest placeholders; replacement values themselves stay as OsString.
    let Some(mut rest) = value.to_str() else {
        return Ok(value.to_owned());
    };
    let mut result = OsString::new();
    while let Some(start) = rest.find("${") {
        result.push(&rest[..start]);
        rest = &rest[start + 2..];
        let end = rest.find('}').ok_or(ArgumentError::UnterminatedVariable)?;
        let name = &rest[..end];
        if let Some(value) = variables.get(name) {
            result.push(value);
        } else if policy == UnknownVariablePolicy::Preserve {
            result.push(format!("${{{name}}}"));
        } else {
            return Err(ArgumentError::UnknownVariable { name: name.into() });
        }
        rest = &rest[end + 1..];
    }
    result.push(rest);
    Ok(result)
}

pub(super) fn expand_all(
    values: Vec<OsString>,
    variables: &BTreeMap<String, OsString>,
    policy: UnknownVariablePolicy,
) -> Result<Vec<OsString>, ArgumentError> {
    values
        .iter()
        .map(|value| expand(value, variables, policy))
        .collect()
}
