//! Legacy plain-label ordering keys; these are never emitted as display output.

use mcpm_lib::local::mcmod::{
    VersionConstraint, VersionOperator,
    dependency::{
        Dependency, DependencyOrdering, DependencySide, graph::candidate::DependencyCandidate,
    },
};
fn text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() {
                ch.escape_default().to_string()
            } else {
                ch.to_string()
            }
        })
        .collect()
}
pub(super) fn candidate(candidate: &DependencyCandidate) -> String {
    match candidate {
        DependencyCandidate::External(candidate) => candidate.version.as_ref().map_or_else(
            || "[external]".to_owned(),
            |version| format!("[external @{}]", text(version)),
        ),
        DependencyCandidate::PrimaryMod(candidate) => {
            format!("[primary @{}]", text(&candidate.modinfo.version))
        }
        DependencyCandidate::NestedMod(candidate) => {
            format!("[nested @{}]", text(&candidate.modinfo.version))
        }
    }
}
pub(super) fn details(dependency: &Dependency) -> String {
    let mut value = version(&dependency.version_requirement);
    let mut rules = Vec::new();
    if let Some(side) = dependency.side {
        rules.push(format!(
            "side={}",
            match side {
                DependencySide::Client => "client",
                DependencySide::Server => "server",
                DependencySide::Both => "both",
            }
        ));
    }
    if let Some(ordering) = dependency.ordering {
        rules.push(format!(
            "ordering={}",
            match ordering {
                DependencyOrdering::None => "none",
                DependencyOrdering::BeforeTarget => "before target",
                DependencyOrdering::AfterTarget => "after target",
            }
        ));
    }
    if let Some(reason) = &dependency.reason {
        rules.push(format!("reason={}", text(reason)));
    }
    if !rules.is_empty() {
        value.push_str(&format!(" [{}]", rules.join("; ")));
    }
    value
}
fn version(constraint: &VersionConstraint) -> String {
    match constraint {
        VersionConstraint::Any => "*".into(),
        VersionConstraint::Compare { operator, version } => {
            let operator = match operator {
                VersionOperator::Equal => "=",
                VersionOperator::NotEqual => "!=",
                VersionOperator::Greater => ">",
                VersionOperator::GreaterOrEqual => ">=",
                VersionOperator::Less => "<",
                VersionOperator::LessOrEqual => "<=",
            };
            format!("{operator}{}", text(version))
        }
        VersionConstraint::Wildcard(pattern) => text(pattern),
        VersionConstraint::Caret(value) => format!("^{}", text(value)),
        VersionConstraint::Tilde(value) => format!("~{}", text(value)),
        VersionConstraint::Recommended(value) => format!("recommended({})", text(value)),
        VersionConstraint::All(conditions) => group(conditions.iter().map(version), " && ", "*"),
        VersionConstraint::AnyOf(conditions) => {
            group(conditions.iter().map(version), " || ", "<no version>")
        }
    }
}
fn group(parts: impl Iterator<Item = String>, separator: &str, empty: &str) -> String {
    let parts = parts.collect::<Vec<_>>();
    match parts.len() {
        0 => empty.into(),
        1 => parts.into_iter().next().unwrap(),
        _ => format!("({})", parts.join(separator)),
    }
}
