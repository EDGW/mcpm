//! Stable declaration hierarchy and progressive expansion for the mod browser.

use super::super::{
    ListedFile,
    rows::{self, Row, RowOptions},
};
use crate::commands::supports::mods::mod_records;

#[derive(Debug)]
pub(super) struct Node {
    pub row: Row,
    pub file: usize,
    pub outer: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: usize,
    pub expanded: bool,
}

pub(super) fn build(files: &[ListedFile], options: &RowOptions<'_>) -> Vec<Node> {
    let mut nodes = Vec::new();
    for (number, listed) in files.iter().enumerate() {
        let mut settings = RowOptions {
            expanded: false,
            paths: options.paths,
            index_type: options.index_type,
        };
        let summary = rows::for_file(listed, number + 1, &settings).remove(0);
        settings.expanded = true;
        let mut full = rows::for_file(listed, number + 1, &settings).into_iter();
        // The file summary represents its first declaration, avoiding duplicate rows.
        let _ = full.next();
        let root = nodes.len();
        let records: Vec<_> = listed
            .scanned
            .file
            .metadata
            .iter()
            .enumerate()
            .flat_map(|(metadata, value)| {
                mod_records::selected(value, metadata, "", listed.records.as_deref())
                    .into_iter()
                    .map(move |record| (metadata, record.nested_indices))
            })
            .collect();
        push(
            &mut nodes,
            summary,
            None,
            options.expanded,
            number,
            records.first().is_none_or(|(_, path)| path.is_empty()),
        );
        let mut positions = vec![root];
        for (index, row) in full.enumerate() {
            let record_index = index + 1;
            let parent = records
                .get(record_index)
                .and_then(|(metadata, path)| {
                    records[..record_index]
                        .iter()
                        .enumerate()
                        .filter(|(_, (candidate_metadata, candidate_path))| {
                            candidate_metadata == metadata
                                && candidate_path.len() < path.len()
                                && path.starts_with(candidate_path)
                        })
                        .max_by_key(|(index, (_, path))| (path.len(), std::cmp::Reverse(*index)))
                        .map(|(index, _)| positions[index])
                })
                .unwrap_or(root);
            let position = nodes.len();
            push(
                &mut nodes,
                row,
                Some(parent),
                options.expanded,
                number,
                records
                    .get(record_index)
                    .is_none_or(|(_, path)| path.is_empty()),
            );
            positions.push(position);
        }
    }
    nodes
}

fn push(
    nodes: &mut Vec<Node>,
    row: Row,
    parent: Option<usize>,
    expanded: bool,
    file: usize,
    outer: bool,
) {
    let depth = parent.map_or(0, |parent| nodes[parent].depth + 1);
    let position = nodes.len();
    if let Some(parent) = parent {
        nodes[parent].children.push(position);
    }
    nodes.push(Node {
        row,
        file,
        outer,
        parent,
        children: Vec::new(),
        depth,
        expanded,
    });
}
