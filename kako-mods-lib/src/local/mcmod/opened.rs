//! Common mod identification metadata supplied independently by loaders.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::local::{
    mcmod::{ModEntry, dependency::Dependency},
    modloader::{LoaderId, ModLoaderMeta},
};

/// A mod file with independent metadata records from its loaders.
#[derive(Debug)]
pub struct ModFile {
    /// The file's path, filename, and enabled state.
    pub entry: ModEntry,
    /// One metadata record per loader that recognizes this file.
    /// Records remain separate even when their mod IDs match.
    pub metadata: Vec<ModMeta>,
}

/// Common identification metadata supplied by one loader for one mod file.
///
/// The supplying loader interprets its source format and supplies these common
/// properties. A file can declare multiple mods through a single loader.
#[derive(Debug)]
pub struct ModMeta {
    /// The loader that supplied this record, including its instance.
    pub loader: ModLoaderMeta,
    /// Only the mods declared directly in this JAR according to the supplying loader.
    pub mods: Vec<ModInfo>,
    /// JARs declared directly inside this JAR, in declaration order.
    /// Every child metadata record must use the same loader as this record.
    pub nested: Vec<NestedModMeta>,
}

impl ModMeta {
    /// Whether this JAR or any declared descendant contains recognized mods.
    pub fn has_mods(&self) -> bool {
        !self.mods.is_empty() || self.nested.iter().any(|child| child.meta.has_mods())
    }
}

/// One declared child JAR and the supplying loader's metadata for it.
#[derive(Debug)]
pub struct NestedModMeta {
    /// Archive member path relative to the immediate parent JAR.
    /// Does not contain an outer filesystem path or synthetic `!` separators.
    pub path: String,
    /// Metadata supplied by the same loader as the parent record.
    /// May contain no direct mods while still having nested JARs.
    pub meta: ModMeta,
}

/// The minimum identification properties of one declared mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    /// Display name supplied by the loader, falling back to the mod ID when absent.
    pub name: String,
    /// The mod identifier reported by the supplying loader.
    pub modid: String,
    /// The version string, with loader-supported variable references resolved.
    /// Ordinary strings retain their original values without version parsing.
    /// A loader may supply its format's default when the property is absent.
    pub version: String,
    /// Dependencies reported by the loader; empty when there are none.
    pub dependencies: Vec<Dependency>,
}

/// The caller's decision after a loader identification attempt fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentificationErrorAction {
    /// Try the same loader again. The caller must bound retries if desired.
    Retry,
    /// Keep successful records and continue with the next loader.
    Skip,
    /// Return the current error without returning partial metadata.
    Abort,
}

/// Identifies a mod entry using each loader in the supplied map.
///
/// Loader iteration order is unspecified. Each successful identification adds a
/// separate metadata record; loaders that return `None` are skipped.
/// If no loader recognizes the entry, the returned file has empty metadata.
/// The entry and loader map are consumed, and the file is not modified.
/// On failure, `on_error` receives the entry, loader, attempt number (starting
/// at one for each loader), and the error with loader/path context. It may retry,
/// skip, or abort. Skipped errors are the caller's responsibility to record.
/// An empty metadata list can mean no recognition or that all errors were skipped.
///
/// # Errors
///
/// Returns the original contextual error when `on_error` selects
/// [`IdentificationErrorAction::Abort`]. No partial file is returned in that case.
pub fn open<F>(
    entry: ModEntry,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    mut on_error: F,
) -> Result<ModFile>
where
    F: FnMut(&ModEntry, &ModLoaderMeta, usize, &anyhow::Error) -> IdentificationErrorAction,
{
    let mut metadata = Vec::new();

    for loader in loaders.values() {
        let mut attempt = 1;
        loop {
            let identified = loader.instance().try_identify(&entry).with_context(|| {
                format!(
                    "loader {:?} failed to identify mod file {:?}",
                    loader.id(),
                    entry.path()
                )
            });
            match identified {
                Ok(identified) => {
                    if let Some(identified) = identified {
                        metadata.push(identified);
                    }
                    break;
                }
                Err(error) => match on_error(&entry, loader, attempt, &error) {
                    IdentificationErrorAction::Retry => attempt = attempt.saturating_add(1),
                    IdentificationErrorAction::Skip => break,
                    IdentificationErrorAction::Abort => return Err(error),
                },
            }
        }
    }

    Ok(ModFile { entry, metadata })
}
