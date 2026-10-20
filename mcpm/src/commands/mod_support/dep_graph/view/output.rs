//! Renders dependency trees after rule filtering, retaining paths to matching descendants.

use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
};

use kako_mods_lib::local::mcmod::dependency::{
    Dependency, DependencyKind, DependencyMatch,
    graph::{DependencyGraph, candidate::DependencyCandidate, identifier::DependencyIdentifier},
};

use crate::commands::{
    mod_support::dependency_output::{format_details, text},
    output_style::OutputStyle,
};

use super::{filter::visibility::Visibility, matching};
use crate::theme::{self, Role};
use owo_colors::Style;

#[derive(Clone, Copy)]
enum Entry<'a> {
    Node {
        identifier: &'a DependencyIdentifier,
        dependency: Option<&'a Dependency>,
        source_candidate: Option<&'a DependencyCandidate>,
        target: Option<&'a DependencyIdentifier>,
        depth: usize,
    },
    Candidate {
        dependency: Option<&'a Dependency>,
        candidate: &'a DependencyCandidate,
        depth: usize,
    },
}

// Explicit tasks avoid recursion limits when displaying long chains.
enum Task<'a> {
    Visit {
        entry: Entry<'a>,
        prefix: String,
        branch: Option<(String, &'static str)>,
    },
    Leave(&'a DependencyIdentifier),
}

/// Uses the renderer's child expansion so isolation follows the actual displayed tree.
pub(super) fn has_children(
    graph: &DependencyGraph,
    root: &DependencyIdentifier,
    visibility: &Visibility,
    reverse: bool,
) -> bool {
    !children(
        graph,
        Entry::Node {
            identifier: root,
            dependency: None,
            source_candidate: None,
            target: None,
            depth: 0,
        },
        visibility,
        reverse,
    )
    .is_empty()
}

pub(super) fn write_view(
    output: &mut impl Write,
    graph: &DependencyGraph,
    root: &DependencyIdentifier,
    depths: HashMap<DependencyIdentifier, usize>,
    visibility: &Visibility,
    reverse: bool,
) -> io::Result<()> {
    let style = OutputStyle::new();
    if !graph.identification_failures().is_empty() {
        writeln!(
            output,
            "{}",
            style.warning(format!(
                "Identification: incomplete ({} failed loader attempts)",
                graph.identification_failures().len()
            ))
        )?;
    }
    let mut expanded = HashSet::new();
    let mut active = HashSet::new();
    let mut pending = vec![Task::Visit {
        entry: Entry::Node {
            identifier: root,
            dependency: None,
            source_candidate: None,
            target: None,
            depth: 0,
        },
        prefix: String::new(),
        branch: None,
    }];
    while let Some(task) = pending.pop() {
        let Task::Visit {
            entry,
            prefix,
            branch,
        } = task
        else {
            if let Task::Leave(identifier) = task {
                active.remove(identifier);
            }
            continue;
        };
        if let Some((prefix, branch)) = branch {
            write!(output, "{}{}", style.muted(prefix), style.muted(branch))?;
        }
        let mut folded = None;
        if let Entry::Node {
            identifier, depth, ..
        } = entry
        {
            if active.contains(identifier) {
                folded = Some("cycle");
            } else if !reverse
                && (depths[identifier] < depth || !expanded.insert(identifier.clone()))
            {
                folded = Some("shared");
            }
        }
        let label = label(graph, entry, folded, &style, reverse);
        writeln!(output, "{label}")?;
        let children = if folded.is_some() {
            Vec::new()
        } else {
            children(graph, entry, visibility, reverse)
        };
        if children.is_empty() {
            continue;
        }
        if let Entry::Node { identifier, .. } = entry {
            active.insert(identifier.clone());
            pending.push(Task::Leave(identifier));
        }
        let count = children.len();
        for (index, child) in children.into_iter().enumerate().rev() {
            let last = index + 1 == count;
            let child_prefix = format!("{prefix}{}", if last { "     " } else { "│    " });
            pending.push(Task::Visit {
                entry: child,
                prefix: child_prefix,
                branch: Some((
                    prefix.clone(),
                    if last {
                        "└─── "
                    } else {
                        "├─── "
                    },
                )),
            });
        }
    }
    Ok(())
}

fn label(
    graph: &DependencyGraph,
    entry: Entry<'_>,
    folded: Option<&str>,
    style: &OutputStyle,
    reverse: bool,
) -> String {
    match entry {
        Entry::Candidate {
            candidate,
            dependency,
            ..
        } => {
            let state = dependency.map(|dependency| {
                matching::candidate_match(candidate, candidate.identifier().loader(), dependency)
            });
            candidate_label(
                candidate,
                dependency.zip(state).map(|(d, s)| (d.kind, s)),
                style,
            )
        }
        Entry::Node {
            identifier,
            dependency,
            source_candidate,
            target,
            depth,
        } => {
            let node = graph
                .node(identifier)
                .expect("validated target node exists");
            let prefix = if depth == 0 {
                style.loader(
                    identifier.loader().as_str(),
                    &format!("{}:", text(identifier.loader().as_str())),
                )
            } else {
                String::new()
            };
            let mut value = format!("[{prefix}{}]", style.important(text(identifier.id())));
            let state = dependency.map(|dependency| {
                let target = target.unwrap_or(identifier);
                (dependency.kind, matching::target(graph, target, dependency))
            });
            if let Some(candidate) = source_candidate {
                value.push_str(&candidate_suffix(candidate, state, style));
            } else {
                match node.candidates() {
                    [] => value.push_str(&style.warning(" [no candidate]")),
                    [candidate] => value.push_str(&candidate_suffix(candidate, state, style)),
                    candidates => {
                        value.push_str(&style.normal(format!(" [{} candidates]", candidates.len())))
                    }
                }
            }
            if let Some(dependency) = dependency {
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
                let marker = format!(" [{folded}]");
                value.push_str(&if folded == "cycle" {
                    style.warning(marker)
                } else {
                    style.muted(marker)
                });
            }
            value
        }
    }
}

/// Tree relation labels have their own palette, separate from info/deps output.
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
    candidate: &DependencyCandidate,
    state: Option<(DependencyKind, DependencyMatch)>,
    style: &OutputStyle,
) -> String {
    let value = match candidate {
        DependencyCandidate::External(candidate) => candidate.version.as_ref().map_or_else(
            || " [external]".to_owned(),
            |version| format!("@{} [external]", text(version)),
        ),
        DependencyCandidate::PrimaryMod(candidate) => {
            format!("@{}", text(&candidate.modinfo.version))
        }
        DependencyCandidate::NestedMod(candidate) => {
            format!("@{} [nested]", text(&candidate.modinfo.version))
        }
    };
    paint_candidate(value, state, style)
}

fn candidate_label(
    candidate: &DependencyCandidate,
    state: Option<(DependencyKind, DependencyMatch)>,
    style: &OutputStyle,
) -> String {
    let value = match candidate {
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
    };
    paint_candidate(value, state, style)
}

fn paint_candidate(
    value: String,
    state: Option<(DependencyKind, DependencyMatch)>,
    style: &OutputStyle,
) -> String {
    let paint = match state {
        None => theme::console(Role::Normal),
        Some((_, DependencyMatch::Unknown)) => theme::console(Role::Warning),
        Some((kind, state)) => {
            let incompatible = matches!(kind, DependencyKind::Breaks | DependencyKind::Conflicts);
            if (state == DependencyMatch::Matched) != incompatible {
                theme::console(Role::Success)
            } else {
                theme::console(Role::Danger)
            }
        }
    };
    style.paint(value, paint)
}

fn children<'a>(
    graph: &'a DependencyGraph,
    entry: Entry<'a>,
    visibility: &Visibility,
    reverse: bool,
) -> Vec<Entry<'a>> {
    match entry {
        Entry::Candidate {
            candidate, depth, ..
        } => edges(candidate, depth, visibility),
        Entry::Node {
            identifier,
            depth,
            dependency,
            ..
        } => {
            if reverse {
                return reverse_edges(graph, identifier, depth, visibility);
            }
            let candidates = graph
                .node(identifier)
                .expect("validated target node exists")
                .candidates();
            if let [candidate] = candidates {
                return edges(candidate, depth, visibility);
            }
            let mut candidates = candidates.iter().map(|c| c.as_ref()).collect::<Vec<_>>();
            if dependency.map_or_else(
                || visibility.filtered(),
                |dependency| !visibility.own(dependency),
            ) {
                candidates.retain(|candidate| visibility.candidate(candidate));
            }
            candidates
                .sort_by_key(|candidate| candidate_label(candidate, None, &OutputStyle::plain()));
            candidates
                .into_iter()
                .map(|candidate| Entry::Candidate {
                    candidate,
                    depth,
                    dependency,
                })
                .collect()
        }
    }
}

fn edges<'a>(
    candidate: &'a DependencyCandidate,
    depth: usize,
    visibility: &Visibility,
) -> Vec<Entry<'a>> {
    let mut edges = candidate
        .outgoing()
        .filter(|(_, dependency)| visibility.edge(dependency))
        .collect::<Vec<_>>();
    edges.sort_by_key(|(target, dependency)| {
        (
            relation_order(dependency.kind),
            target.id(),
            format_details(dependency, &OutputStyle::plain()),
        )
    });
    edges
        .into_iter()
        .map(|(identifier, dependency)| Entry::Node {
            identifier,
            dependency: Some(dependency),
            source_candidate: None,
            target: None,
            depth: depth + 1,
        })
        .collect()
}

fn reverse_edges<'a>(
    graph: &'a DependencyGraph,
    identifier: &'a DependencyIdentifier,
    depth: usize,
    visibility: &Visibility,
) -> Vec<Entry<'a>> {
    let mut edges = graph.incoming_relations(identifier);
    edges.retain(|(_, _, dependency)| visibility.edge(dependency));
    edges.sort_by_key(|(source, candidate, dependency)| {
        (
            relation_order(dependency.kind),
            source.id(),
            candidate_label(candidate, None, &OutputStyle::plain()),
            format_details(dependency, &OutputStyle::plain()),
        )
    });
    edges
        .into_iter()
        .map(|(source, candidate, dependency)| Entry::Node {
            identifier: source,
            dependency: Some(dependency),
            source_candidate: Some(candidate),
            target: Some(identifier),
            depth: depth + 1,
        })
        .collect()
}

fn relation_order(kind: DependencyKind) -> usize {
    match kind {
        DependencyKind::Required => 0,
        DependencyKind::Optional => 1,
        DependencyKind::Recommended => 2,
        DependencyKind::Suggested => 3,
        DependencyKind::Breaks => 4,
        DependencyKind::Conflicts => 5,
    }
}
