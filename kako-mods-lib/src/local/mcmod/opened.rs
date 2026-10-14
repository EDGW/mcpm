//! Common mod identification metadata supplied independently by loaders.

use anyhow::{Context, Result};

use crate::local::{
    mcmod::{DependencyConstraint, ModEntry},
    modloader::{ModLoaderMeta, ModLoaderSet},
};

/// A mod file with independent metadata records from its loaders.
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
pub struct ModMeta {
    /// The loader that supplied this record, including its instance.
    pub loader: ModLoaderMeta,
    /// The mods declared in this file according to the supplying loader,
    /// including nested declarations that the loader identifies.
    pub mods: Vec<ModInfo>,
}

/// The minimum identification properties of one declared mod.
#[derive(Debug, Clone)]
pub struct ModInfo {
    /// Archive member paths from the outer file to this mod's declaring JAR.
    /// Empty for declarations in the outer file; one element per nesting level.
    pub nested_path: Vec<String>,
    /// Display name supplied by the loader, falling back to the mod ID when absent.
    pub name: String,
    /// The mod identifier reported by the supplying loader.
    pub modid: String,
    /// The version string, with loader-supported variable references resolved.
    /// Ordinary strings retain their original values without version parsing.
    /// A loader may supply its format's default when the property is absent.
    pub version: String,
    /// Dependencies reported by the loader; empty when there are none.
    pub dependencies: Vec<DependencyConstraint>,
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

/// Identifies a mod entry using each loader in the supplied set.
///
/// Loaders are tried in insertion order. Each successful identification adds a
/// separate metadata record in that order; loaders that return `None` are skipped.
/// If no loader recognizes the entry, the returned file has empty metadata.
/// The entry and loader set are consumed, and the file is not modified.
/// On failure, `on_error` receives the entry, loader, attempt number (starting
/// at one for each loader), and the error with loader/path context. It may retry,
/// skip, or abort. Skipped errors are the caller's responsibility to record.
/// An empty metadata list can mean no recognition or that all errors were skipped.
///
/// # Errors
///
/// Returns the original contextual error when `on_error` selects
/// [`IdentificationErrorAction::Abort`]. No partial file is returned in that case.
pub fn open<F>(entry: ModEntry, loaders: ModLoaderSet, mut on_error: F) -> Result<ModFile>
where
    F: FnMut(&ModEntry, &ModLoaderMeta, usize, &anyhow::Error) -> IdentificationErrorAction,
{
    let mut metadata = Vec::new();

    for loader in loaders.iter() {
        let mut attempt = 1;
        loop {
            let identified = loader.instance.try_identify(&entry).with_context(|| {
                format!(
                    "loader {:?} failed to identify mod file {:?}",
                    loader.id,
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
