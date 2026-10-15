use std::str::FromStr;

use clap::{Args, ValueEnum};
use kako_mods_lib::local::mcmod::dependency::{
    Dependency, DependencyKind, graph::identifier::DependencyIdentifier,
};
use thiserror::Error;

#[derive(Args)]
pub(super) struct FilterArgs {
    /// Ignore a displayed neighboring mod; repeat to add more. Use loader:id, :id, or id.
    #[arg(long, value_name = "MOD")]
    ignore: Vec<IgnoreSelector>,
    /// Ignore comma-separated targets; combined with --ignore and default ignores.
    #[arg(long, value_delimiter = ',', value_name = "MODS")]
    ignores: Vec<IgnoreSelector>,
    /// Disable the default ignore for minecraft; explicit ignores still apply.
    #[arg(long)]
    no_default_ignores: bool,
    /// Show this relation type; repeat to select more. Overrides the default types.
    #[arg(long = "type", value_enum, value_name = "TYPE")]
    kind: Vec<RelationType>,
    /// Show comma-separated relation types; combined with --type.
    #[arg(
        long = "types",
        value_enum,
        value_delimiter = ',',
        value_name = "TYPES"
    )]
    kinds: Vec<RelationType>,
}

#[derive(Clone, Copy, ValueEnum)]
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

pub(super) struct ViewFilter {
    ignores: Vec<IgnoreSelector>,
    ignore_minecraft: bool,
    kinds: Vec<DependencyKind>,
}

impl FilterArgs {
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
        ViewFilter {
            ignores: self.ignore.into_iter().chain(self.ignores).collect(),
            ignore_minecraft: !self.no_default_ignores,
            kinds,
        }
    }
}

impl ViewFilter {
    /// Filters declared edges only; an explicitly selected root remains visible.
    pub(super) fn includes(&self, target: &DependencyIdentifier, dependency: &Dependency) -> bool {
        self.kinds.contains(&dependency.kind)
            && !(self.ignore_minecraft && target.id() == "minecraft")
            && !self.ignores.iter().any(|ignore| ignore.matches(target))
    }
}
