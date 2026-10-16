//! Shared Forge/NeoForge dependency predicates using Maven version ordering.

use crate::local::mcmod::{
    ModInfo, VersionConstraint,
    dependency::{Dependency, DependencyMatch},
};

use super::super::utils::dependency;

mod version;

pub(super) fn matches(candidate: &ModInfo, required: &Dependency) -> DependencyMatch {
    dependency::matches(candidate, required, |constraint| {
        // Any/All/AnyOf are evaluated by the shared composer before this atomic callback.
        match constraint {
            // A bare Maven version is a recommendation with an unrestricted range.
            VersionConstraint::Recommended(_) => DependencyMatch::Matched,
            VersionConstraint::Compare { operator, version } => {
                // Both FML loaders explicitly accept their development/missing-version sentinel.
                if candidate.version == "0.0NONE" {
                    return DependencyMatch::Matched;
                }
                match version::compare(&candidate.version, version) {
                    Some(ordering) => dependency::compare(ordering, *operator),
                    None => DependencyMatch::Unknown,
                }
            }
            // These operators have no native Maven meaning.
            VersionConstraint::Wildcard(_)
            | VersionConstraint::Caret(_)
            | VersionConstraint::Tilde(_) => DependencyMatch::Unknown,
            VersionConstraint::Any | VersionConstraint::All(_) | VersionConstraint::AnyOf(_) => {
                unreachable!("the shared composer evaluates unrestricted and compound constraints")
            }
        }
    })
}
