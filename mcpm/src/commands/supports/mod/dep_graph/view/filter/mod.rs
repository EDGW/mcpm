//! Base edge filters and per-relation match rules for dependency tree presentation.

use std::str::FromStr;

use clap::{Args, ValueEnum};
use kako_mods_lib::local::mcmod::dependency::{
    Dependency, DependencyKind, graph::identifier::DependencyIdentifier,
};
use thiserror::Error;

mod rule;
pub(super) mod visibility;
use rule::Rule;

#[derive(Args, Default, Debug)]
pub(super) struct FilterArgs {
    /// Ignore a displayed neighboring mod; repeat to add more. Use loader:id, :id, or id.
    #[arg(short = 'g', long, value_name = "MOD")]
    ignore: Vec<IgnoreSelector>,
    /// Ignore comma-separated targets; combined with --ignore and default ignores.
    #[arg(short = 'G', long, value_delimiter = ',', value_name = "MODS")]
    ignores: Vec<IgnoreSelector>,
    /// Disable the default ignore for minecraft; explicit ignores still apply.
    #[arg(short = 'X', long)]
    no_default_ignores: bool,
    /// Show this relation type; repeat to select more. Overrides the default types.
    #[arg(short = 't', long = "type", value_enum, value_name = "TYPE")]
    kind: Vec<RelationType>,
    /// Show comma-separated relation types; combined with --type.
    #[arg(
        short = 'T',
        long = "types",
        value_enum,
        value_delimiter = ',',
        value_name = "TYPES"
    )]
    kinds: Vec<RelationType>,
    /// Match states for TYPE; defaults to all. Last rule per type wins, overriding --problem.
    #[arg(short = 'r', long = "rule", value_name = "TYPE:MATCH[,MATCH...]")]
    rules: Vec<Rule>,
    /// Shortcut for required:unmatched,unknown; breaks/conflicts:matched,unknown; hide isolated roots; disable default ignores.
    #[arg(short = 'p', long)]
    problem: bool,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum RelationType {
    Required,
    Optional,
    Recommended,
    Suggested,
    #[value(alias = "break")]
    Breaks,
    #[value(alias = "conflict")]
    Conflicts,
}

impl RelationType {
    fn kind(self) -> DependencyKind {
        match self {
            Self::Required => DependencyKind::Required,
            Self::Optional => DependencyKind::Optional,
            Self::Recommended => DependencyKind::Recommended,
            Self::Suggested => DependencyKind::Suggested,
            Self::Breaks => DependencyKind::Breaks,
            Self::Conflicts => DependencyKind::Conflicts,
        }
    }
}

#[derive(Debug, Clone)]
struct IgnoreSelector {
    loader: Option<String>,
    id: String,
}

#[derive(Debug, Error)]
#[error("invalid ignore selector {value:?}: expected loader:id, :id, or id with a nonempty id")]
struct InvalidIgnoreSelector {
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

#[derive(Debug)]
pub(super) struct ViewFilter {
    ignores: Vec<IgnoreSelector>,
    ignore_minecraft: bool,
    kinds: Vec<DependencyKind>,
    rules: Vec<Rule>,
}

impl FilterArgs {
    pub(super) fn problem() -> Self {
        Self {
            problem: true,
            ..Self::default()
        }
    }

    pub(super) fn hides_isolated_roots(&self) -> bool {
        self.problem
    }

    pub(super) fn into_filter(self) -> ViewFilter {
        let kinds = if self.kind.is_empty() && self.kinds.is_empty() {
            vec![
                DependencyKind::Required,
                DependencyKind::Breaks,
                DependencyKind::Conflicts,
            ]
        } else {
            self.kind
                .into_iter()
                .chain(self.kinds)
                .map(RelationType::kind)
                .collect()
        };
        let mut rules = if self.problem {
            Rule::problem()
        } else {
            Vec::new()
        };
        // Explicit rules override the shortcut; the last rule for a type wins.
        rules.extend(self.rules);
        ViewFilter {
            ignores: self.ignore.into_iter().chain(self.ignores).collect(),
            ignore_minecraft: !(self.no_default_ignores || self.problem),
            kinds,
            rules,
        }
    }
}

impl ViewFilter {
    pub(super) fn accepts(
        &self,
        kind: DependencyKind,
        state: kako_mods_lib::local::mcmod::dependency::DependencyMatch,
    ) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.kind == kind)
            .is_none_or(|rule| rule.accepts(state))
    }

    pub(super) fn has_rules(&self) -> bool {
        self.kinds.iter().any(|kind| !self.unrestricted(*kind))
    }

    pub(super) fn unrestricted(&self, kind: DependencyKind) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.kind == kind)
            .is_none_or(Rule::is_all)
    }

    /// Filters declared edges only; an explicitly selected root remains visible.
    pub(super) fn includes(&self, target: &DependencyIdentifier, dependency: &Dependency) -> bool {
        self.kinds.contains(&dependency.kind)
            && !(self.ignore_minecraft && target.id() == "minecraft")
            && !self.ignores.iter().any(|ignore| ignore.matches(target))
    }
}
