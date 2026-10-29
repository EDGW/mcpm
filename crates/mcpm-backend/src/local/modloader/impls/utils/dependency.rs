//! Shared three-valued constraint composition; atomic version semantics belong to loaders.

use std::cmp::Ordering;

use crate::local::mcmod::{
    ModInfo, VersionConstraint, VersionOperator,
    dependency::{Dependency, DependencyMatch},
};

/// Match declared identities and compose constraints without assuming version semantics.
pub(in crate::local::modloader::impls) fn matches(
    candidate: &ModInfo,
    dependency: &Dependency,
    atom: impl Fn(&VersionConstraint) -> DependencyMatch,
) -> DependencyMatch {
    if candidate.modid.is_empty() || dependency.id.is_empty() {
        return DependencyMatch::Unknown;
    }
    if candidate.modid != dependency.id {
        return DependencyMatch::Unmatched;
    }
    evaluate(&dependency.version_requirement, &atom, 0)
}

fn evaluate(
    constraint: &VersionConstraint,
    atom: &impl Fn(&VersionConstraint) -> DependencyMatch,
    depth: usize,
) -> DependencyMatch {
    use DependencyMatch::{Matched, Unknown, Unmatched};
    if depth >= 128 {
        return Unknown;
    }
    match constraint {
        VersionConstraint::Any => Matched,
        VersionConstraint::All(conditions) | VersionConstraint::AnyOf(conditions) => {
            let all = matches!(constraint, VersionConstraint::All(_));
            let decisive = if all { Unmatched } else { Matched };
            let mut unknown = false;
            for condition in conditions {
                match evaluate(condition, atom, depth + 1) {
                    result if result == decisive => return decisive,
                    Unknown => unknown = true,
                    _ => {}
                }
            }
            if unknown {
                Unknown
            } else if all {
                Matched
            } else {
                Unmatched
            }
        }
        VersionConstraint::Compare { .. }
        | VersionConstraint::Wildcard(_)
        | VersionConstraint::Caret(_)
        | VersionConstraint::Tilde(_)
        | VersionConstraint::Recommended(_) => atom(constraint),
    }
}

pub(in crate::local::modloader::impls) fn outcome(matches: bool) -> DependencyMatch {
    if matches {
        DependencyMatch::Matched
    } else {
        DependencyMatch::Unmatched
    }
}

pub(in crate::local::modloader::impls) fn compare(
    ordering: Ordering,
    operator: VersionOperator,
) -> DependencyMatch {
    outcome(match operator {
        VersionOperator::Equal => ordering.is_eq(),
        VersionOperator::NotEqual => !ordering.is_eq(),
        VersionOperator::Greater => ordering.is_gt(),
        VersionOperator::GreaterOrEqual => !ordering.is_lt(),
        VersionOperator::Less => ordering.is_lt(),
        VersionOperator::LessOrEqual => !ordering.is_gt(),
    })
}

/// Java String.compareTo compares UTF-16 code units, not UTF-8 bytes.
pub(in crate::local::modloader::impls) fn lexical(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}
