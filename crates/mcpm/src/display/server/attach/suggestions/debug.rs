//! `@debug` syntax checker backed by `attach::debug::VARIABLES` as the single source of truth.

use super::{
    super::debug::VARIABLES,
    syntax::{
        Candidate, CandidateEmphasis, Diagnostic, SyntaxChecker, SyntaxQuery,
        boolean_argument_query, next_token,
    },
};
use std::borrow::Cow;

/// Argument completer and advisory validator for `@debug <var_name> <true|false>`.
pub(super) struct DebugSyntax;

impl DebugSyntax {
    fn variable_candidates() -> Vec<Candidate<'static>> {
        VARIABLES
            .iter()
            .map(|entry| Candidate {
                text: entry.name,
                description: Some(entry.description),
                emphasis: CandidateEmphasis::Primary,
                append_space: true,
            })
            .collect()
    }
}

impl SyntaxChecker for DebugSyntax {
    fn summary(&self) -> &str {
        "<var_name> <true|false>"
    }

    fn query<'a, 'input>(&'a self, arguments: &'input str) -> SyntaxQuery<'a, 'input> {
        let Some(first) = next_token(arguments, 0) else {
            return SyntaxQuery {
                candidates: Self::variable_candidates(),
                prefix: "",
                replacement: 0..0,
                diagnostic: None,
            };
        };

        let known = VARIABLES.iter().any(|entry| entry.name == first.text);
        if !known {
            let prefix_valid = VARIABLES
                .iter()
                .any(|entry| entry.name.starts_with(first.text));
            let diagnostic = if first.complete || !prefix_valid {
                Some(Diagnostic {
                    range: first.range.clone(),
                    message: Cow::Borrowed("Unknown debug variable"),
                })
            } else {
                None
            };
            return SyntaxQuery {
                candidates: Self::variable_candidates(),
                prefix: first.text,
                replacement: first.range,
                diagnostic,
            };
        }

        if !first.complete {
            return SyntaxQuery {
                candidates: Self::variable_candidates(),
                prefix: first.text,
                replacement: first.range,
                diagnostic: None,
            };
        }

        boolean_argument_query(arguments, first.range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_debug_variables_and_boolean_values() {
        let syntax = DebugSyntax;

        let initial = syntax.query("");
        assert!(initial.diagnostic.is_none());
        let vars: Vec<_> = initial.matches().collect();
        assert_eq!(vars.len(), VARIABLES.len());
        assert_eq!(vars[0].text, "display_content_range");
        assert!(vars[0].description.is_some());
        assert!(vars[0].append_space);

        assert!(syntax.query("display_").diagnostic.is_none());
        assert!(syntax.query("display_ ").diagnostic.is_some());
        let unknown = syntax.query("nope").diagnostic.unwrap();
        assert_eq!(&"nope"[unknown.range], "nope");
        assert_eq!(unknown.message, "Unknown debug variable");

        let val_prompt = syntax.query("display_content_range ");
        assert!(val_prompt.diagnostic.is_none());
        assert_eq!(
            val_prompt.matches().map(|c| c.text).collect::<Vec<_>>(),
            ["true", "false"]
        );

        let typing = syntax.query("display_content_range f");
        assert!(typing.diagnostic.is_none());
        assert_eq!(
            typing.matches().map(|c| c.text).collect::<Vec<_>>(),
            ["false"]
        );

        let invalid = syntax.query("display_content_range yes").diagnostic.unwrap();
        assert_eq!(&"display_content_range yes"[invalid.range], "yes");
        assert_eq!(invalid.message, "Expected true or false");

        let extra = syntax
            .query("display_content_range true extra")
            .diagnostic
            .unwrap();
        assert_eq!(&"display_content_range true extra"[extra.range], "extra");
        assert_eq!(extra.message, "Unexpected extra argument");
    }
}
