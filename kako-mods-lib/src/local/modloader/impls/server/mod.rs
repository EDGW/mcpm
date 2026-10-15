pub(super) mod fml;

use anyhow::{Result, ensure};
use std::path::Path;

/// Extracts a version only from a complete referenced Maven artifact path.
pub(super) fn coordinate(path: &Path, group: &str, artifact: &str) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let marker = format!("/{}/{artifact}/", group.replace('.', "/"));
    let (_, tail) = text.rsplit_once(&marker)?;
    let (version, file) = tail.split_once('/')?;
    if version.is_empty()
        || file.contains('/')
        || !(file == format!("{artifact}-{version}.jar")
            || (file.starts_with(&format!("{artifact}-{version}-")) && file.ends_with(".jar")))
    {
        return None;
    }
    Some(version.to_owned())
}

pub(super) fn agree(current: &mut Option<String>, value: &str, description: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "empty {description}");
    if let Some(existing) = current {
        ensure!(
            existing == value,
            "conflicting {description}: {existing:?} versus {value:?}"
        );
    } else {
        *current = Some(value.to_owned());
    }
    Ok(())
}
