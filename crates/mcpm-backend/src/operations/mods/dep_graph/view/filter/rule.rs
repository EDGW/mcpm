//! Parsing and default expansion of per-relation match-state rules.

use std::str::FromStr;

use super::super::super::MatchState;
use mcpm_lib::local::mcmod::dependency::{DependencyKind, DependencyMatch};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub kind: DependencyKind,
    pub states: Vec<MatchState>,
}

impl Rule {
    pub fn accepts(&self, state: DependencyMatch) -> bool {
        self.states.contains(&state.into())
    }

    pub fn is_all(&self) -> bool {
        [
            DependencyMatch::Matched,
            DependencyMatch::Unmatched,
            DependencyMatch::Unknown,
        ]
        .into_iter()
        .all(|state| self.accepts(state))
    }

    pub fn problem() -> Vec<Self> {
        use DependencyKind::{Breaks, Conflicts, Required};
        use MatchState::{Matched, Unknown, Unmatched};
        vec![
            Self {
                kind: Required,
                states: vec![Unmatched, Unknown],
            },
            Self {
                kind: Breaks,
                states: vec![Matched, Unknown],
            },
            Self {
                kind: Conflicts,
                states: vec![Matched, Unknown],
            },
        ]
    }
}

impl FromStr for Rule {
    type Err = InvalidRule;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidRule(value.to_owned());
        let (kind, states) = value.split_once(':').ok_or_else(invalid)?;
        let kind = match kind {
            "required" => DependencyKind::Required,
            "optional" => DependencyKind::Optional,
            "recommended" => DependencyKind::Recommended,
            "suggested" => DependencyKind::Suggested,
            "breaks" | "break" => DependencyKind::Breaks,
            "conflicts" | "conflict" => DependencyKind::Conflicts,
            _ => return Err(invalid()),
        };
        let mut parsed = Vec::new();
        for state in states.split(',') {
            match state {
                "matched" => parsed.push(MatchState::Matched),
                "unmatched" => parsed.push(MatchState::Unmatched),
                "unknown" => parsed.push(MatchState::Unknown),
                "all" => parsed.extend([
                    MatchState::Matched,
                    MatchState::Unmatched,
                    MatchState::Unknown,
                ]),
                _ => return Err(invalid()),
            }
        }
        Ok(Self {
            kind,
            states: parsed,
        })
    }
}

#[derive(Debug, Error)]
#[error("invalid dependency rule {0:?}: expected TYPE:matched,unmatched,unknown or TYPE:all")]
pub struct InvalidRule(String);
