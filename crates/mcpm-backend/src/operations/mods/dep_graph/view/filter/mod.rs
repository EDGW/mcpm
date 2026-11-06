//! Node-problem selection and per-relation match rules, independent of CLI and output.

use mcpm_lib::local::mcmod::dependency::{
    Dependency, DependencyKind, DependencyMatch, graph::identifier::DependencyIdentifier,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
mod rule;
pub(super) mod visibility;
pub use rule::{InvalidRule, Rule};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FilterInput {
    pub ignores: Vec<IgnoreSelector>,
    pub no_default_ignores: bool,
    pub kinds: Vec<DependencyKind>,
    pub rules: Vec<Rule>,
    pub problem: bool,
    /// None inherits --problem; explicit false disables duplicate-Primary selection.
    #[serde(default)]
    pub duplicate_primary: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreSelector {
    loader: Option<String>,
    id: String,
}
#[derive(Debug, Error)]
#[error("invalid ignore selector {value:?}: expected loader:id, :id, or id with a nonempty id")]
pub struct InvalidIgnoreSelector {
    value: String,
}
impl FromStr for IgnoreSelector {
    type Err = InvalidIgnoreSelector;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (loader, id) = match value.split_once(':') {
            Some((loader, id)) => ((!loader.is_empty()).then(|| loader.to_owned()), id),
            None => (None, value),
        };
        if id.is_empty() {
            return Err(InvalidIgnoreSelector {
                value: value.into(),
            });
        }
        Ok(Self {
            loader,
            id: id.into(),
        })
    }
}
impl IgnoreSelector {
    fn matches(&self, target: &DependencyIdentifier) -> bool {
        self.id == target.id()
            && self
                .loader
                .as_deref()
                .is_none_or(|loader| loader == target.loader().as_str())
    }
}
pub(super) struct ViewFilter {
    ignores: Vec<IgnoreSelector>,
    ignore_minecraft: bool,
    kinds: Vec<DependencyKind>,
    rules: Vec<Rule>,
    duplicate_primary: bool,
    select_relations: bool,
}
impl FilterInput {
    pub(super) fn into_filter(self) -> ViewFilter {
        let duplicate_primary = self.duplicate_primary.unwrap_or(self.problem);
        let select_relations = !duplicate_primary || self.problem || !self.rules.is_empty();
        let kinds = if self.kinds.is_empty() {
            vec![
                DependencyKind::Required,
                DependencyKind::Breaks,
                DependencyKind::Conflicts,
            ]
        } else {
            self.kinds
        };
        let mut rules = if self.problem {
            Rule::problem()
        } else {
            Vec::new()
        };
        rules.extend(self.rules);
        ViewFilter {
            ignores: self.ignores,
            ignore_minecraft: !(self.no_default_ignores || self.problem),
            kinds,
            rules,
            duplicate_primary,
            select_relations,
        }
    }
}
impl ViewFilter {
    pub(super) fn selects_duplicate_primary(&self, node: &DependencyIdentifier) -> bool {
        self.duplicate_primary && self.includes_node(node)
    }

    pub(super) fn selects_relation(
        &self,
        state: impl FnOnce() -> DependencyMatch,
        kind: DependencyKind,
    ) -> bool {
        self.select_relations && (self.unrestricted(kind) || self.accepts(kind, state()))
    }

    pub(super) fn accepts(&self, kind: DependencyKind, state: DependencyMatch) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.kind == kind)
            .is_none_or(|rule| rule.accepts(state))
    }
    pub(super) fn has_rules(&self) -> bool {
        self.duplicate_primary || self.kinds.iter().any(|kind| !self.unrestricted(*kind))
    }
    pub(super) fn unrestricted(&self, kind: DependencyKind) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.kind == kind)
            .is_none_or(Rule::is_all)
    }
    pub(super) fn includes(&self, target: &DependencyIdentifier, dependency: &Dependency) -> bool {
        self.kinds.contains(&dependency.kind) && self.includes_node(target)
    }

    fn includes_node(&self, target: &DependencyIdentifier) -> bool {
        !(self.ignore_minecraft && target.id() == "minecraft")
            && !self.ignores.iter().any(|ignore| ignore.matches(target))
    }
}
