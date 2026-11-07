//! CLI node-problem and relation filters converted into backend graph inspection input.

use clap::{Args, ValueEnum};
use mcpm_backend::operations::mods::dep_graph::{FilterInput, IgnoreSelector, Rule};
use mcpm_lib::local::mcmod::dependency::DependencyKind;

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
    /// Show dependency problems and duplicate Primary candidates; hide nonproblem isolated roots and disable default ignores.
    #[arg(short = 'p', long)]
    problem: bool,
    /// Select nodes with multiple Primary candidates and paths leading to them; union with --rule.
    #[arg(long, conflicts_with = "no_duplicate_primary")]
    duplicate_primary: bool,
    /// Disable duplicate-Primary selection, including its --problem default; labels remain visible.
    #[arg(long, conflicts_with = "duplicate_primary")]
    no_duplicate_primary: bool,
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

impl FilterArgs {
    pub(super) fn into_input(self) -> FilterInput {
        FilterInput {
            ignores: self.ignore.into_iter().chain(self.ignores).collect(),
            no_default_ignores: self.no_default_ignores,
            kinds: self
                .kind
                .into_iter()
                .chain(self.kinds)
                .map(RelationType::kind)
                .collect(),
            rules: self.rules,
            problem: self.problem,
            duplicate_primary: if self.duplicate_primary {
                Some(true)
            } else if self.no_duplicate_primary {
                Some(false)
            } else {
                None
            },
        }
    }
}
