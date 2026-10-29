//! Compatibility summaries reported by CurseForge project responses, without per-project file requests.

use std::collections::HashSet;

use super::api;

/// Recent indexes are a summary, not an exhaustive history of project files.
pub(super) fn compatibility(project: &api::Project) -> (Vec<String>, Vec<String>) {
    let mut versions = Vec::new();
    let mut loaders = Vec::new();
    for index in project
        .latest_files_indexes
        .iter()
        .chain(&project.latest_early_access_files_indexes)
    {
        versions.push(index.game_version.clone());
        if let Some(loader) = index.mod_loader.and_then(loader_name) {
            loaders.push(loader.to_owned());
        }
    }
    for file in &project.latest_files {
        for tag in &file.sortable_game_versions {
            if tag.game_version_type_id == Some(68441) {
                loaders.push(tag.game_version_name.to_ascii_lowercase());
            } else if !tag.game_version.is_empty() {
                versions.push(tag.game_version.clone());
            }
        }
    }
    (unique(versions), unique(loaders))
}

pub(super) fn unique(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty() && seen.insert(value.clone()))
        .collect()
}

fn loader_name(id: u32) -> Option<&'static str> {
    match id {
        1 => Some("forge"),
        3 => Some("liteloader"),
        4 => Some("fabric"),
        5 => Some("quilt"),
        6 => Some("neoforge"),
        _ => None,
    }
}
