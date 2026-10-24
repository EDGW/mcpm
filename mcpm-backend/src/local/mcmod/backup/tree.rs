//! File-only inventories, safe relative names and staged directory replacement.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::manifest::validate_path;
use crate::local::files::FileIdentity;

pub(super) struct Inventory {
    pub files: Vec<String>,
    pub directories: Vec<PathBuf>,
}

pub(super) fn scan(root: &Path) -> Result<Inventory> {
    let mut inventory = Inventory {
        files: Vec::new(),
        directories: Vec::new(),
    };
    visit(root, root, &mut inventory)?;
    inventory.files.sort_unstable();
    Ok(inventory)
}

fn visit(root: &Path, path: &Path, inventory: &mut Inventory) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "expected a regular directory: {path:?}"
    );
    inventory.directories.push(path.to_owned());
    for entry in fs::read_dir(path).with_context(|| format!("failed to enumerate {path:?}"))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            visit(root, &entry.path(), inventory)?;
        } else {
            ensure!(
                kind.is_file(),
                "expected a regular file or directory: {:?}",
                entry.path()
            );
            let entry_path = entry.path();
            let relative = entry_path.strip_prefix(root)?;
            let components = relative
                .iter()
                .map(|part| part.to_str().context("backup paths must be UTF-8"))
                .collect::<Result<Vec<_>>>()?;
            let relative = components.join("/");
            validate_path(&relative)?;
            inventory.files.push(relative);
        }
    }
    Ok(())
}

/// Validates each parent again instead of following a substituted directory link.
pub(super) fn file_path(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_path(relative)?;
    let mut path = root.to_owned();
    let components: Vec<_> = relative.split('/').collect();
    for component in &components[..components.len() - 1] {
        path.push(component);
        ensure!(
            fs::symlink_metadata(&path)?.is_dir(),
            "expected a regular directory: {path:?}"
        );
    }
    path.push(components.last().unwrap());
    Ok(path)
}

pub(super) fn root(path: &Path) -> Result<PathBuf> {
    // Remove trailing separators and `.` so lstat cannot follow a final symlink.
    let path = path.components().as_path();
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "expected a regular directory: {path:?}"
    );
    fs::canonicalize(path).with_context(|| format!("failed to resolve directory {path:?}"))
}

pub(super) fn disjoint(a: &Path, b: &Path) -> Result<()> {
    ensure!(
        !a.starts_with(b) && !b.starts_with(a),
        "directories overlap: {a:?} and {b:?}"
    );
    Ok(())
}

/// Remembers the original directory identity so a changed destination is refused.
pub(super) struct Destination {
    pub path: PathBuf,
    original: Option<FileIdentity>,
    force: bool,
}

impl Destination {
    pub fn inspect(path: &Path, force: bool) -> Result<Self> {
        let filename = path
            .file_name()
            .context("destination must have a directory name")?;
        ensure!(
            filename != "." && filename != "..",
            "invalid destination {path:?}"
        );
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = fs::canonicalize(parent)
            .with_context(|| format!("destination parent does not exist: {parent:?}"))?;
        let path = parent.join(filename);
        let original = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir(),
                    "expected a regular destination directory: {path:?}"
                );
                Some(FileIdentity::read(&path)?)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("failed to inspect destination"),
        };
        let destination = Self {
            path,
            original,
            force,
        };
        destination.validate()?;
        Ok(destination)
    }

    fn validate(&self) -> Result<()> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir() && self.original == Some(FileIdentity::read(&self.path)?),
                    "destination changed during backup operation: {:?}",
                    self.path
                );
                ensure!(
                    self.force || fs::read_dir(&self.path)?.next().transpose()?.is_none(),
                    "destination is not empty; apply with --force to replace it: {:?}",
                    self.path
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ensure!(
                    self.original.is_none(),
                    "destination disappeared: {:?}",
                    self.path
                );
            }
            Err(error) => return Err(error).context("failed to recheck destination"),
        }
        Ok(())
    }

    /// Keeps the previous tree until publication succeeds. Recovery failures preserve it.
    pub fn commit(&self, staged: &Path, holding: &Path) -> Result<()> {
        self.validate()?;
        if self.original.is_some() {
            fs::rename(&self.path, holding).context("failed to retain previous destination")?;
        }
        if let Err(error) = fs::rename(staged, &self.path) {
            let publication =
                anyhow::Error::new(error).context("failed to publish backup operation");
            if self.original.is_some() {
                if let Err(rollback) = fs::rename(holding, &self.path) {
                    return Err(publication.context(rollback).context(format!(
                        "failed to restore destination; previous files remain at {holding:?}"
                    )));
                }
            }
            return Err(publication);
        }
        Ok(())
    }
}

/// Rejects names that cannot be represented faithfully by the current filesystem.
pub(super) fn validate_target_names(files: &[super::FileRecord]) -> Result<()> {
    #[cfg(windows)]
    {
        use std::collections::BTreeMap;
        let mut components = BTreeMap::new();
        for file in files {
            let mut prefix = String::new();
            for name in file.path().split('/') {
                ensure!(
                    !name.ends_with([' ', '.'])
                        && !name.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c)),
                    "file name is not supported on Windows: {name:?}"
                );
                let stem = name.split('.').next().unwrap().to_uppercase();
                ensure!(
                    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                        && !(stem.len() == 4
                            && (stem.starts_with("COM") || stem.starts_with("LPT"))
                            && stem.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])),
                    "reserved Windows file name {name:?}"
                );
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(name);
                if let Some(previous) = components.insert(prefix.to_uppercase(), prefix.clone()) {
                    ensure!(
                        previous == prefix,
                        "case-insensitive backup path collision: {previous:?} and {prefix:?}"
                    );
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = files;
    Ok(())
}
