//! Terminal-only dependency tree labels, colors, branches and indentation.

use crate::{
    display::{
        dependency::{format_details, text},
        style::OutputStyle,
    },
    theme::{self, Role},
};
use anyhow::{Context, Result};
use mcpm_backend::operations::mods::dep_graph::{
    CandidateSummary, CandidateView, Fold, MatchState, TreeLabel, ViewOutput,
};
use mcpm_lib::local::mcmod::dependency::DependencyKind;
use owo_colors::Style;
use std::io::Write;

pub(super) fn write_graph(view: &ViewOutput, output: &mut impl Write) -> Result<usize> {
    let style = OutputStyle::new();
    let mut ancestors = Vec::new();
    for (tree_index, tree) in view.trees.iter().enumerate() {
        if tree_index > 0 {
            writeln!(output).context("failed to separate dependency graph views")?;
        }
        if view.identification_failures > 0 {
            writeln!(
                output,
                "{}",
                style.warning(format!(
                    "Identification: incomplete ({} failed loader attempts)",
                    view.identification_failures
                ))
            )
            .context("failed to write dependency graph view")?;
        }
        for entry in tree {
            if let Some(parent) = entry.parent {
                ancestors.clear();
                let mut ancestor = &tree[parent];
                while let Some(parent) = ancestor.parent {
                    ancestors.push(ancestor.last);
                    ancestor = &tree[parent];
                }
                for &last in ancestors.iter().rev() {
                    write!(
                        output,
                        "{}",
                        style.muted(if last { "     " } else { "│    " })
                    )?;
                }
                write!(
                    output,
                    "{}",
                    style.muted(if entry.last {
                        "└─── "
                    } else {
                        "├─── "
                    })
                )?;
            }
            writeln!(output, "{}", label(&entry.label, &style, view.reverse))
                .context("failed to write dependency graph view")?;
        }
    }
    output
        .flush()
        .context("failed to flush dependency graph view")?;
    Ok(view.trees.len())
}
fn label(entry: &TreeLabel, style: &OutputStyle, reverse: bool) -> String {
    match entry {
        TreeLabel::Candidate {
            candidate,
            relation,
            state,
        } => candidate_label(
            candidate,
            relation.as_ref().zip(*state).map(|(d, s)| (d.kind, s)),
            style,
        ),
        TreeLabel::Node {
            identifier,
            root,
            candidates,
            duplicate_primary,
            relation,
            state,
            folded,
        } => {
            let prefix = if *root {
                style.loader(
                    &identifier.loader,
                    &format!("{}:", text(&identifier.loader)),
                )
            } else {
                String::new()
            };
            let mut value = format!("[{prefix}{}]", style.important(text(&identifier.id)));
            let state = relation
                .as_ref()
                .zip(*state)
                .map(|(dependency, state)| (dependency.kind, state));
            match candidates {
                CandidateSummary::Missing => value.push_str(&style.warning(" [no candidate]")),
                CandidateSummary::One(candidate) => {
                    value.push_str(&candidate_suffix(candidate, state, style))
                }
                CandidateSummary::Multiple(count) => {
                    value.push_str(&style.normal(format!(" [{count} candidates]")))
                }
            }
            if let Some(count) = duplicate_primary {
                value.push_str(&style.danger(format!(" [duplicate primary: {count}]")));
            }
            if let Some(dependency) = relation {
                value = if reverse {
                    format!(
                        "({} by){value} {}{}{}",
                        format_kind(dependency.kind, style),
                        style.muted("(parent: "),
                        format_details(dependency, style),
                        style.muted(")")
                    )
                } else {
                    format!(
                        "({}){value} {}",
                        format_kind(dependency.kind, style),
                        format_details(dependency, style)
                    )
                };
            }
            if let Some(folded) = folded {
                match folded {
                    Fold::Cycle => value.push_str(&style.warning(" [cycle]")),
                    Fold::Shared => value.push_str(&style.muted(" [shared]")),
                }
            }
            value
        }
    }
}
fn format_kind(kind: DependencyKind, style: &OutputStyle) -> String {
    let (label, paint) = match kind {
        DependencyKind::Required => ("required", Style::new()),
        DependencyKind::Optional => ("optional", theme::console(Role::Muted)),
        DependencyKind::Recommended => ("recommended", theme::console(Role::Muted)),
        DependencyKind::Suggested => ("suggested", theme::console(Role::Muted)),
        DependencyKind::Breaks => ("breaks", theme::console(Role::Danger)),
        DependencyKind::Conflicts => ("conflicts", theme::console(Role::Danger)),
    };
    style.paint(label, paint)
}
fn candidate_suffix(
    candidate: &CandidateView,
    state: Option<(DependencyKind, MatchState)>,
    style: &OutputStyle,
) -> String {
    let value = match candidate {
        CandidateView::External { version } => version.as_ref().map_or_else(
            || " [external]".to_owned(),
            |version| format!("@{} [external]", text(version)),
        ),
        CandidateView::Primary { version } => format!("@{}", text(version)),
        CandidateView::Nested { version } => format!("@{} [nested]", text(version)),
    };
    paint_candidate(value, state, style)
}
fn candidate_label(
    candidate: &CandidateView,
    state: Option<(DependencyKind, MatchState)>,
    style: &OutputStyle,
) -> String {
    let value = match candidate {
        CandidateView::External { version } => version.as_ref().map_or_else(
            || "[external]".to_owned(),
            |version| format!("[external @{}]", text(version)),
        ),
        CandidateView::Primary { version } => format!("[primary @{}]", text(version)),
        CandidateView::Nested { version } => format!("[nested @{}]", text(version)),
    };
    paint_candidate(value, state, style)
}
fn paint_candidate(
    value: String,
    state: Option<(DependencyKind, MatchState)>,
    style: &OutputStyle,
) -> String {
    let paint = match state {
        None => theme::console(Role::Normal),
        Some((_, MatchState::Unknown)) => theme::console(Role::Warning),
        Some((kind, state)) => {
            let incompatible = matches!(kind, DependencyKind::Breaks | DependencyKind::Conflicts);
            if (state == MatchState::Matched) != incompatible {
                theme::console(Role::Success)
            } else {
                theme::console(Role::Danger)
            }
        }
    };
    style.paint(value, paint)
}
