//! Candidate and target matching shared by graph rule filtering and display colors.

use kako_mods_lib::local::{
    mcmod::{
        ModInfo,
        dependency::{
            Dependency, DependencyMatch,
            graph::{
                DependencyGraph, candidate::DependencyCandidate, identifier::DependencyIdentifier,
            },
        },
    },
    modloader::{BUILTIN_LOADERS, LoaderId},
};

/// Any matching candidate wins, then uncertainty, then no match; missing targets are unmatched.
pub(super) fn target(
    graph: &DependencyGraph,
    target: &DependencyIdentifier,
    dependency: &Dependency,
) -> DependencyMatch {
    let mut result = DependencyMatch::Unmatched;
    for candidate in graph
        .node(target)
        .expect("declared targets have graph nodes")
        .candidates()
    {
        match candidate_match(candidate, target.loader(), dependency) {
            DependencyMatch::Matched => return DependencyMatch::Matched,
            DependencyMatch::Unknown => result = DependencyMatch::Unknown,
            DependencyMatch::Unmatched => {}
        }
    }
    result
}

/// Versioned candidates use their loader's rules; unversioned externals match identity alone.
pub(super) fn candidate_match(
    candidate: &DependencyCandidate,
    loader: &LoaderId,
    dependency: &Dependency,
) -> DependencyMatch {
    let identity = candidate.identifier();
    if identity.loader() != loader || identity.id() != dependency.id {
        return DependencyMatch::Unmatched;
    }
    match candidate {
        DependencyCandidate::PrimaryMod(candidate) => {
            dependency.matches(&candidate.modinfo, candidate.loader.instance().as_ref())
        }
        DependencyCandidate::NestedMod(candidate) => {
            dependency.matches(&candidate.modinfo, candidate.loader.instance().as_ref())
        }
        DependencyCandidate::External(candidate) => {
            let Some(version) = &candidate.version else {
                return DependencyMatch::Matched;
            };
            let Some(loader) = BUILTIN_LOADERS.get(candidate.identifier.loader()) else {
                return DependencyMatch::Unknown;
            };
            dependency.matches(
                &ModInfo {
                    name: candidate.identifier.id().to_owned(),
                    modid: candidate.identifier.id().to_owned(),
                    version: version.clone(),
                    dependencies: Vec::new(),
                },
                loader.instance().as_ref(),
            )
        }
    }
}
