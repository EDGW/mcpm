//! Fabric dependency predicates, including its string fallback and wildcard bounds.
//!
//! Fabric caret keeps the major component even for 0.x; tilde keeps both major
//! and minor even for a one-component operand. These are Fabric rules, not npm's.

use crate::local::mcmod::{
    ModInfo, VersionConstraint, VersionOperator,
    dependency::{Dependency, DependencyMatch},
};

use super::super::utils::dependency;
use version::SemanticVersion;

mod version;

pub(super) fn matches(candidate: &ModInfo, required: &Dependency) -> DependencyMatch {
    let semantic = SemanticVersion::parse(&candidate.version, false);
    dependency::matches(candidate, required, |constraint| {
        if candidate.version.is_empty() {
            return DependencyMatch::Unknown;
        }
        match constraint {
            VersionConstraint::Compare { operator, version } => {
                compare(&candidate.version, semantic.as_ref(), version, *operator)
            }
            VersionConstraint::Caret(version) | VersionConstraint::Tilde(version) => compatible(
                &candidate.version,
                semantic.as_ref(),
                version,
                matches!(constraint, VersionConstraint::Tilde(_)),
            ),
            VersionConstraint::Wildcard(pattern) => {
                wildcard(&candidate.version, semantic.as_ref(), pattern)
            }
            // Native Maven recommendations have no Fabric meaning.
            VersionConstraint::Recommended(_) => DependencyMatch::Unknown,
            VersionConstraint::Any | VersionConstraint::All(_) | VersionConstraint::AnyOf(_) => {
                unreachable!("the shared composer evaluates unrestricted and compound constraints")
            }
        }
    })
}

fn compare(
    raw: &str,
    candidate: Option<&SemanticVersion<'_>>,
    required: &str,
    operator: VersionOperator,
) -> DependencyMatch {
    if required.is_empty() {
        return DependencyMatch::Unknown;
    }
    let reference = SemanticVersion::parse(required, true);
    if reference
        .as_ref()
        .is_some_and(|v| v.components.last() == Some(&-1))
    {
        return match operator {
            VersionOperator::Equal => wildcard(raw, candidate, required),
            // Fabric rejects ordered wildcard predicates.
            _ => DependencyMatch::Unknown,
        };
    }
    match (candidate, reference.as_ref()) {
        (Some(a), Some(b)) => dependency::compare(a.compare(b), operator),
        _ => {
            // Non-semantic reference versions only support inclusive operators;
            // Fabric normalizes all of those predicates to exact string equality.
            if reference.is_none()
                && matches!(operator, VersionOperator::Greater | VersionOperator::Less)
            {
                return DependencyMatch::Unknown;
            }
            let equal = friendly(raw, candidate) == friendly(required, reference.as_ref());
            dependency::outcome(match operator {
                VersionOperator::Greater | VersionOperator::Less => false,
                VersionOperator::NotEqual => !equal,
                _ => equal,
            })
        }
    }
}

fn compatible(
    raw: &str,
    candidate: Option<&SemanticVersion<'_>>,
    required: &str,
    minor: bool,
) -> DependencyMatch {
    if required.is_empty() {
        return DependencyMatch::Unknown;
    }
    let reference = SemanticVersion::parse(required, true);
    if reference
        .as_ref()
        .is_some_and(|v| v.components.last() == Some(&-1))
    {
        return DependencyMatch::Unknown;
    }
    match (candidate, reference.as_ref()) {
        (Some(a), Some(b)) => dependency::outcome(
            !a.compare(b).is_lt()
                && a.component(0) == b.component(0)
                && (!minor || a.component(1) == b.component(1)),
        ),
        _ => {
            dependency::outcome(friendly(raw, candidate) == friendly(required, reference.as_ref()))
        }
    }
}

fn wildcard(raw: &str, candidate: Option<&SemanticVersion<'_>>, pattern: &str) -> DependencyMatch {
    if pattern == "*" {
        return DependencyMatch::Matched;
    }
    if matches!(pattern, "x" | "X") {
        return dependency::outcome(raw == pattern);
    }
    let Some(mut lower) = SemanticVersion::parse(pattern, true) else {
        return DependencyMatch::Unknown;
    };
    if lower.components.last() != Some(&-1) {
        return DependencyMatch::Unknown;
    }
    lower.components.pop();
    // Fabric's x-ranges start at the empty prerelease, including prereleases.
    lower.prerelease = Some("");
    let Some(candidate) = candidate else {
        return DependencyMatch::Unmatched;
    };
    let same_prefix = lower
        .components
        .iter()
        .enumerate()
        .all(|(index, value)| candidate.component(index) == *value);
    dependency::outcome(same_prefix && !candidate.compare(&lower).is_lt())
}

fn friendly(raw: &str, version: Option<&SemanticVersion<'_>>) -> String {
    version.map_or_else(|| raw.to_owned(), SemanticVersion::friendly)
}
