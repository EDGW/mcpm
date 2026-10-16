use std::{
    io::{self, Write},
    path::Path,
};

use kako_mods_lib::local::mc::client::{
    CheckOptionValue, CheckReport, CheckTarget, FileCheckStatus,
};

use super::super::output::{error_chain, text};
use crate::{commands::output_style::OutputStyle, terminal};

pub(super) fn write_report(output: &mut impl Write, report: &CheckReport) -> io::Result<()> {
    let formatted = terminal::stdout_is_tty();
    let style = OutputStyle::new();
    let state = if report.is_intact() {
        "intact"
    } else if report.has_errors() {
        "failed"
    } else {
        "unverified"
    };
    let verified = report
        .files
        .iter()
        .filter(|file| file.status == FileCheckStatus::Verified)
        .count();
    let failed = report
        .files
        .iter()
        .filter(|file| file.status == FileCheckStatus::Failed)
        .count();
    let mode_name = |mode| match mode {
        CheckOptionValue::Size => "size",
        CheckOptionValue::Hash => "hash",
        CheckOptionValue::Skip => "skip",
    };
    let scope = [
        ("libraries", report.options.libraries),
        ("assets", report.options.assets),
        ("asset_index", report.options.asset_index),
        ("version_jar", report.options.version_jar),
        ("logging", report.options.logging),
    ]
    .into_iter()
    .filter(|(_, mode)| mode.is_enabled())
    .map(|(name, mode)| format!("{name}:{}", mode_name(mode)))
    .collect::<Vec<_>>()
    .join(",");
    let summary = format!(
        "scope={scope}, files={}, verified={verified}, unverified={}, failed={failed}, issues={}",
        report.files.len(),
        report.unverified_count(),
        report.issues.len()
    );
    if formatted {
        let badge = format!("[{state}]");
        let badge = match state {
            "intact" => style.required(badge),
            "failed" => style.danger(badge),
            _ => style.warning(badge),
        };
        writeln!(output, "{} {badge}", style.important(text(&report.version)))?;
        writeln!(output, "  {}", style.secondary(&summary))?;
    } else {
        raw_row(output, report, state, "summary", None, &summary)?;
    }
    for issue in &report.issues {
        let target = target_name(&issue.target);
        let reason = text(&error_chain(&issue.problem));
        if formatted {
            writeln!(
                output,
                "  {} {}",
                style.danger("[failed]"),
                style.important(text(&target))
            )?;
            if let Some(path) = &issue.path {
                writeln!(
                    output,
                    "    {}",
                    style.secondary(text(&path.display().to_string()))
                )?;
            }
            writeln!(output, "    {}", style.warning(reason))?;
        } else {
            raw_row(
                output,
                report,
                "failed",
                &target,
                issue.path.as_deref(),
                &reason,
            )?;
        }
    }
    for file in report
        .files
        .iter()
        .filter(|file| file.status == FileCheckStatus::Unverified)
    {
        let target = target_name(&file.target);
        let reason = match file.mode {
            CheckOptionValue::Size => "no size metadata",
            CheckOptionValue::Hash => "no SHA-1 metadata",
            CheckOptionValue::Skip => "skipped",
        };
        if formatted {
            writeln!(
                output,
                "  {} {}",
                style.warning("[unverified]"),
                style.important(text(&target))
            )?;
            writeln!(
                output,
                "    {}",
                style.secondary(text(&file.path.display().to_string()))
            )?;
            writeln!(
                output,
                "    {}",
                style.warning(format!(
                    "{reason}; cannot complete the requested verification."
                ))
            )?;
        } else {
            raw_row(
                output,
                report,
                "unverified",
                &target,
                Some(&file.path),
                reason,
            )?;
        }
    }
    if formatted {
        writeln!(output)?;
    }
    Ok(())
}

fn raw_row(
    output: &mut impl Write,
    report: &CheckReport,
    state: &str,
    target: &str,
    path: Option<&Path>,
    detail: &str,
) -> io::Result<()> {
    writeln!(
        output,
        "{}\t{state}\t{}\t{}\t{}",
        text(&report.version),
        text(target),
        path.map(|path| text(&path.display().to_string()))
            .unwrap_or_default(),
        text(detail)
    )
}

fn target_name(target: &CheckTarget) -> String {
    match target {
        CheckTarget::Version => "version".into(),
        CheckTarget::VersionJar => "version JAR".into(),
        CheckTarget::Logging { id } => format!("logging config {id}"),
        CheckTarget::Library { name, classifier } => match classifier {
            Some(classifier) => format!("library {name} ({classifier})"),
            None => format!("library {name}"),
        },
        CheckTarget::AssetIndex { id } => format!("asset index {id}"),
        CheckTarget::AssetObject { name } => format!("asset {name}"),
    }
}
