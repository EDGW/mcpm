use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
};

use kako_mods_lib::local::mcmod::dependency::{
    Dependency, DependencyKind,
    graph::{DependencyGraph, candidate::DependencyCandidate, identifier::DependencyIdentifier},
};

use super::super::super::{
    dependency_output::{format_details, format_kind, text},
    output_style::OutputStyle,
};

use super::filter::ViewFilter;

#[derive(Clone, Copy)]
enum Entry<'a> {
    Node {
        identifier: &'a DependencyIdentifier,
        dependency: Option<&'a Dependency>,
        source_candidate: Option<&'a DependencyCandidate>,
        depth: usize,
    },
    Candidate {
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

pub(super) fn write_view(
    output: &mut impl Write,
    graph: &DependencyGraph,
    root: &DependencyIdentifier,
    depths: HashMap<DependencyIdentifier, usize>,
    filter: &ViewFilter,
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
            write!(
                output,
                "{}{}",
                style.secondary(prefix),
                style.secondary(branch)
            )?;
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
            children(graph, entry, filter, reverse)
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
        Entry::Candidate { candidate, .. } => candidate_label(candidate, style),
        Entry::Node {
            identifier,
            dependency,
            source_candidate,
            depth,
        } => {
            let node = graph
                .node(identifier)
                .expect("validated target node exists");
            let id = if depth == 0 {
                identifier.to_string()
            } else {
                identifier.id().to_owned()
            };
            let mut value = format!(
                "{}{}{}",
                style.important("["),
                style.loader(identifier.loader().as_str(), &text(&id)),
                style.important("]")
            );
            if let Some(candidate) = source_candidate {
                value.push_str(&candidate_suffix(candidate, style));
            } else {
                match node.candidates() {
                    [] => value.push_str(&style.warning(" [no candidate]")),
                    [candidate] => value.push_str(&candidate_suffix(candidate, style)),
                    candidates => value
                        .push_str(&style.warning(format!(" [{} candidates]", candidates.len()))),
                }
            }
            if let Some(dependency) = dependency {
                value = if reverse {
                    format!(
                        "({} by){value} {}{}{}",
                        format_kind(dependency.kind, style),
                        style.secondary("(parent: "),
                        format_details(dependency, style),
                        style.secondary(")")
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
                    style.secondary(marker)
                });
            }
            value
        }
    }
}

fn candidate_suffix(candidate: &DependencyCandidate, style: &OutputStyle) -> String {
    match candidate {
        DependencyCandidate::External(_) => style.secondary(" [external]"),
        DependencyCandidate::PrimaryMod(candidate) => {
            style.important(format!("@{}", text(&candidate.modinfo.version)))
        }
        DependencyCandidate::NestedMod(candidate) => format!(
            "{}{}",
            style.important(format!("@{}", text(&candidate.modinfo.version))),
            style.secondary(" [nested]")
        ),
    }
}

fn candidate_label(candidate: &DependencyCandidate, style: &OutputStyle) -> String {
    match candidate {
        DependencyCandidate::External(_) => style.secondary("[external]"),
        DependencyCandidate::PrimaryMod(candidate) => {
            style.important(format!("[primary @{}]", text(&candidate.modinfo.version)))
        }
        DependencyCandidate::NestedMod(candidate) => {
            style.secondary(format!("[nested @{}]", text(&candidate.modinfo.version)))
        }
    }
}

fn children<'a>(
    graph: &'a DependencyGraph,
    entry: Entry<'a>,
    filter: &ViewFilter,
    reverse: bool,
) -> Vec<Entry<'a>> {
    match entry {
        Entry::Candidate { candidate, depth } => edges(candidate, depth, filter),
        Entry::Node {
            identifier, depth, ..
        } => {
            if reverse {
                return reverse_edges(graph, identifier, depth, filter);
            }
            let candidates = graph
                .node(identifier)
                .expect("validated target node exists")
                .candidates();
            if let [candidate] = candidates {
                return edges(candidate, depth, filter);
            }
            let mut candidates = candidates.iter().map(|c| c.as_ref()).collect::<Vec<_>>();
            candidates.sort_by_key(|candidate| candidate_label(candidate, &OutputStyle::plain()));
            candidates
                .into_iter()
                .map(|candidate| Entry::Candidate { candidate, depth })
                .collect()
        }
    }
}

fn edges<'a>(
    candidate: &'a DependencyCandidate,
    depth: usize,
    filter: &ViewFilter,
) -> Vec<Entry<'a>> {
    let mut edges = candidate
        .outgoing()
        .filter(|(target, dependency)| filter.includes(target, dependency))
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
            depth: depth + 1,
        })
        .collect()
}

fn reverse_edges<'a>(
    graph: &'a DependencyGraph,
    identifier: &DependencyIdentifier,
    depth: usize,
    filter: &ViewFilter,
) -> Vec<Entry<'a>> {
    let mut edges = graph.incoming_relations(identifier);
    edges.retain(|(source, _, dependency)| filter.includes(source, dependency));
    edges.sort_by_key(|(source, candidate, dependency)| {
        (
            relation_order(dependency.kind),
            source.id(),
            candidate_label(candidate, &OutputStyle::plain()),
            format_details(dependency, &OutputStyle::plain()),
        )
    });
    edges
        .into_iter()
        .map(|(identifier, candidate, dependency)| Entry::Node {
            identifier,
            dependency: Some(dependency),
            source_candidate: Some(candidate),
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
