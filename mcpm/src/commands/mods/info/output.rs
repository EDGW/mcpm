use std::io::{self, Write};

use kako_mods_lib::local::mcmod::{
    DependencyConstraint, ModFile, ModStatus, VersionConstraint, VersionOperator,
};

pub(super) fn write_info(output: &mut impl Write, file: &ModFile) -> io::Result<()> {
    writeln!(output, "File: {}", file.entry.path().display())?;
    let status = match file.entry.status() {
        ModStatus::Enabled => "enabled",
        ModStatus::Disabled => "disabled",
    };
    writeln!(output, "Status: {status}")?;

    for metadata in &file.metadata {
        writeln!(output, "\nLoader: {}", text(&metadata.loader.name))?;
        for (index, info) in metadata.mods.iter().enumerate() {
            if index > 0 {
                writeln!(output)?;
            }
            writeln!(output, "  Name: {}", text(&info.name))?;
            writeln!(output, "  ID: {}", text(&info.modid))?;
            writeln!(output, "  Version: {}", text(&info.version))?;
            if info.dependencies.is_empty() {
                writeln!(output, "  Dependencies: none")?;
            } else {
                writeln!(output, "  Dependencies:")?;
                for dependency in &info.dependencies {
                    writeln!(output, "    {}", format_dependency(dependency))?;
                }
            }
        }
    }
    Ok(())
}

fn format_dependency(constraint: &DependencyConstraint) -> String {
    match constraint {
        DependencyConstraint::Mod(dependency) => format!(
            "{}: {}",
            text(&dependency.modid),
            format_version(&dependency.version_requirement)
        ),
        DependencyConstraint::All(conditions) => {
            group(conditions.iter().map(format_dependency), " && ", "true")
        }
        DependencyConstraint::AnyOf(conditions) => {
            group(conditions.iter().map(format_dependency), " || ", "false")
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
fn text(value: &str) -> String {
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
