//! Portable relative resource paths and SHA-1 descriptor validation.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

// region: within

/// Joins only portable relative paths. Cache metadata must not escape its root.
pub(super) fn within(root: &Path, value: &str) -> Result<PathBuf, ResourcePathError> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ResourcePathError {
            value: value.to_owned(),
        });
    }
    Ok(root.join(value))
}

#[derive(Debug, Error)]
#[error("invalid relative resource path {value:?}")]
pub struct ResourcePathError {
    pub value: String,
}

// endregion

pub(super) fn sha1(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|ch| ch.is_ascii_hexdigit())
}
