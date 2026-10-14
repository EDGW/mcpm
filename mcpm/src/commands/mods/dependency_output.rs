use kako_mods_lib::local::mcmod::{
    Dependency, DependencyConstraint, DependencyKind, DependencyOrdering, DependencySide,
    VersionConstraint, VersionOperator,
};

pub(super) fn format_dependency(dependency: &Dependency) -> String {
    let kind = match dependency.kind {
        DependencyKind::Required => "required",
        DependencyKind::Optional => "optional",
        DependencyKind::Recommended => "recommended",
        DependencyKind::Suggested => "suggested",
        DependencyKind::Breaks => "breaks",
        DependencyKind::Conflicts => "conflicts",
    };
    format!("{kind}: {}", format_constraint(&dependency.constraint))
}

fn format_constraint(constraint: &DependencyConstraint) -> String {
    match constraint {
        DependencyConstraint::Reference(reference) => {
            let mut value = format!(
                "{}: {}",
                text(&reference.id),
                format_version(&reference.version_requirement)
            );
            let q = &reference.qualifiers;
            let mut rules = Vec::new();
            if q.optional {
                rules.push("if present".into());
            }
            if let Some(unless) = &q.unless {
                rules.push(format!("unless {}", format_constraint(unless)));
            }
            if let Some(side) = q.side {
                rules.push(format!(
                    "side={}",
                    match side {
                        DependencySide::Client => "client",
                        DependencySide::Server => "server",
                        DependencySide::Both => "both",
                    }
                ));
            }
            if let Some(ordering) = q.ordering {
                rules.push(format!(
                    "ordering={}",
                    match ordering {
                        DependencyOrdering::None => "none",
                        DependencyOrdering::BeforeTarget => "before target",
                        DependencyOrdering::AfterTarget => "after target",
                    }
                ));
            }
            if let Some(reason) = &q.reason {
                rules.push(format!("reason={}", text(reason)));
            }
            if !rules.is_empty() {
                value.push_str(&format!(" [{}]", rules.join("; ")));
            }
            value
        }
        DependencyConstraint::All(conditions) => {
            group(conditions.iter().map(format_constraint), " && ", "true")
        }
        DependencyConstraint::AnyOf(conditions) => {
            group(conditions.iter().map(format_constraint), " || ", "false")
        }
    }
}

fn format_version(constraint: &VersionConstraint) -> String {
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
