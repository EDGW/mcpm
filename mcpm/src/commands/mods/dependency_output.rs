use kako_mods_lib::local::mcmod::{
    VersionConstraint, VersionOperator,
    dependency::{Dependency, DependencyKind, DependencyOrdering, DependencySide},
};

use super::output_style::OutputStyle;

pub(super) fn format_kind(kind: DependencyKind, style: &OutputStyle) -> String {
    let label = match kind {
        DependencyKind::Required => "required",
        DependencyKind::Optional => "optional",
        DependencyKind::Recommended => "recommended",
        DependencyKind::Suggested => "suggested",
        DependencyKind::Breaks => "breaks",
        DependencyKind::Conflicts => "conflicts",
    };
    match kind {
        DependencyKind::Required => style.required(label),
        DependencyKind::Recommended => style.recommended(label),
        DependencyKind::Suggested => style.suggested(label),
        DependencyKind::Breaks => style.danger(label),
        DependencyKind::Conflicts => style.warning(label),
        _ => style.secondary(label),
    }
}

pub(super) fn format_dependency(dependency: &Dependency, style: &OutputStyle) -> String {
    format!(
        "{}: {}: {}",
        format_kind(dependency.kind, style),
        style.important(text(&dependency.id)),
        format_details(dependency, style)
    )
}

pub(super) fn format_details(dependency: &Dependency, style: &OutputStyle) -> String {
    let mut value = style.important(format_version(&dependency.version_requirement));
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
        value.push_str(&style.secondary(format!(" [{}]", rules.join("; "))));
    }
    value
}

pub(super) fn format_version(constraint: &VersionConstraint) -> String {
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
        VersionConstraint::Caret(version) => format!("^{}", text(version)),
        VersionConstraint::Tilde(version) => format!("~{}", text(version)),
        VersionConstraint::Recommended(version) => format!("recommended({})", text(version)),
        VersionConstraint::All(conditions) => {
            group(conditions.iter().map(format_version), " && ", "*")
        }
        VersionConstraint::AnyOf(conditions) => group(
            conditions.iter().map(format_version),
            " || ",
            "<no version>",
        ),
    }
}

fn group(parts: impl Iterator<Item = String>, separator: &str, empty: &str) -> String {
    let parts: Vec<_> = parts.collect();
    match parts.len() {
        0 => empty.into(),
        1 => parts.into_iter().next().unwrap(),
        _ => format!("({})", parts.join(separator)),
    }
}

// Keep metadata on its own line, including when it contains terminal controls.
pub(super) fn text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}
