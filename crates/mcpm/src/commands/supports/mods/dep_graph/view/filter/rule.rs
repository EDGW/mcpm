//! Parsing and default expansion of per-relation match-state rules.

use std::str::FromStr;

use mcpm_backend::local::mcmod::dependency::{DependencyKind, DependencyMatch};
use thiserror::Error;

use super::RelationType;

#[derive(Clone, Debug)]
pub(super) struct Rule {
    pub kind: DependencyKind,
    states: Vec<DependencyMatch>,
}

impl Rule {
    pub fn accepts(&self, state: DependencyMatch) -> bool {
        self.states.contains(&state)
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
        use DependencyMatch::{Matched, Unknown, Unmatched};
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
        let kind = <RelationType as clap::ValueEnum>::from_str(kind, false)
            .map_err(|_| invalid())?
            .kind();
        let mut parsed = Vec::new();
        for state in states.split(',') {
            match state {
                "matched" => parsed.push(DependencyMatch::Matched),
                "unmatched" => parsed.push(DependencyMatch::Unmatched),
                "unknown" => parsed.push(DependencyMatch::Unknown),
                "all" => parsed.extend([
                    DependencyMatch::Matched,
                    DependencyMatch::Unmatched,
                    DependencyMatch::Unknown,
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
pub(super) struct InvalidRule(String);
