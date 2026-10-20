//! Best-effort discovery of style collections, isolating malformed files and entries.

use super::Palette;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::Path,
};
use toml::Value;

pub(super) fn styles(documents: &mut BTreeMap<String, Value>, directory: &Path) {
    let path = directory.join("styles");
    let files = match fs::read_dir(&path) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "failed to read style directory");
            return;
        }
    };
    let mut paths = Vec::new();
    for file in files {
        match file {
            Ok(file)
                if file
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("toml")) =>
            {
                paths.push(file.path())
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "failed to read style directory entry"),
        }
    }
    paths.sort_by_key(|path| {
        (
            path.file_name().is_some_and(|name| name == "custom.toml"),
            path.clone(),
        )
    });
    let mut seen = HashSet::new();
    for path in paths {
        let result = (|| -> Result<Value> {
            let document: Value = toml::from_str(&fs::read_to_string(&path)?)?;
            ensure!(
                document.get("style").is_some_and(Value::is_array)
                    || (path.file_name().is_some_and(|name| name == "custom.toml")
                        && document.get("deleted").is_some_and(Value::is_array)),
                "expected [[style]] collection"
            );
            Ok(document)
        })();
        let document = match result {
            Ok(document) => document,
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %format!("{error:#}"), "failed to load style collection");
                continue;
            }
        };
        let entries = document
            .get("style")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let custom = path.file_name().is_some_and(|name| name == "custom.toml");
        let mut local_ids = HashSet::new();
        for entry in entries {
            let result = (|| -> Result<(String, Value)> {
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .context("style ID is missing")?
                    .to_owned();
                ensure!(
                    !local_ids.contains(&id) && (custom || !seen.contains(&id)),
                    "duplicate style ID {id:?}"
                );
                let mut candidate = documents
                    .values()
                    .filter_map(|doc| doc.get("style"))
                    .filter_map(Value::as_array)
                    .flatten()
                    .find(|style| style.get("id").and_then(Value::as_str) == Some(&id))
                    .cloned()
                    .unwrap_or_else(|| Value::Table(Default::default()));
                super::merge(&mut candidate, entry);
                let palette: Palette = candidate.clone().try_into()?;
                ensure!(
                    palette.version == 1,
                    "unsupported style version {}",
                    palette.version
                );
                ensure!(
                    !palette.types.is_empty()
                        && palette
                            .types
                            .iter()
                            .all(|kind| matches!(kind.as_str(), "console" | "tui")),
                    "invalid style types"
                );
                for (_, fallback, kind) in super::default_selections() {
                    ensure!(
                        id != fallback || palette.types.iter().any(|value| value == kind),
                        "default palette must support {kind}"
                    );
                }
                Ok((id, candidate))
            })();
            match result {
                Ok((id, candidate)) => {
                    seen.insert(id.clone());
                    local_ids.insert(id.clone());
                    for doc in documents.values_mut() {
                        if let Some(styles) = doc.get_mut("style").and_then(Value::as_array_mut) {
                            styles.retain(|style| {
                                style.get("id").and_then(Value::as_str) != Some(&id)
                            });
                        }
                    }
                    let name = format!("styles/{}", path.file_name().unwrap().to_string_lossy());
                    let doc = documents
                        .entry(name)
                        .or_insert_with(|| Value::Table(Default::default()));
                    doc.as_table_mut()
                        .unwrap()
                        .entry("style")
                        .or_insert_with(|| Value::Array(Vec::new()))
                        .as_array_mut()
                        .unwrap()
                        .push(candidate);
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), error = %format!("{error:#}"), "failed to load style")
                }
            }
        }
        if custom && let Some(deleted) = document.get("deleted").and_then(Value::as_array) {
            let defaults = super::Config::defaults();
            let deleted: Vec<_> = deleted
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| !defaults.styles.contains_key(*id))
                .map(|id| Value::String(id.into()))
                .collect();
            for doc in documents.values_mut() {
                if let Some(styles) = doc.get_mut("style").and_then(Value::as_array_mut) {
                    styles.retain(|style| !deleted.iter().any(|id| style.get("id") == Some(id)));
                }
            }
            documents
                .entry("styles/custom.toml".into())
                .or_insert_with(|| Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
                .insert("deleted".into(), Value::Array(deleted));
        }
    }
}

/// A skipped style must not turn its saved selection into a startup failure.
pub(super) fn repair_selections(documents: &mut BTreeMap<String, Value>) {
    for (field, fallback, kind) in super::default_selections() {
        let id = documents["settings.toml"]["settings"][field]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let compatible = documents
            .values()
            .filter_map(|doc| doc.get("style"))
            .filter_map(Value::as_array)
            .flatten()
            .any(|style| {
                style.get("id").and_then(Value::as_str) == Some(&id)
                    && style
                        .get("types")
                        .and_then(Value::as_array)
                        .is_some_and(|types| types.iter().any(|value| value.as_str() == Some(kind)))
            });
        if !compatible {
            tracing::warn!(style = %id, "selected style unavailable; using bundled default");
            documents.get_mut("settings.toml").unwrap()["settings"][field] =
                Value::String(fallback);
        }
    }
}
