use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

use thiserror::Error;
use zip::ZipArchive;

use super::{JavaVersionCheck, LaunchPlan};
use crate::local::mc::client::CheckReport;

/// A ready-to-run plan. Prepared native files remain on disk.
#[derive(Debug)]
pub struct PreparedLaunch {
    pub(super) plan: LaunchPlan,
}

#[derive(Debug, Error)]
pub enum PreparationError {
    #[error("{operation}: {path:?}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("required launch file {path:?} is not a regular file")]
    NotFile { path: PathBuf },
    #[error("selected resources did not pass verification")]
    Verification { report: Box<CheckReport> },
    #[error("cannot read native archive {path:?}")]
    Archive {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("native archive {archive:?} has an unsafe entry {entry:?}")]
    UnsafeEntry { archive: PathBuf, entry: String },
    #[error("native extraction exceeds the {maximum_bytes} byte limit")]
    ExtractionLimit { maximum_bytes: u64 },
    #[error("refusing to write through a symlink or non-directory at {path:?}")]
    UnsafeDestination { path: PathBuf },
    #[error("Java version probe did not return a recognizable successful version")]
    JavaVersionUnknown,
    #[error("Java {actual} does not satisfy {policy:?} requirement {required}")]
    JavaVersionMismatch {
        actual: u32,
        required: u32,
        policy: JavaVersionCheck,
    },
}

impl LaunchPlan {
    /// Validates local prerequisites and prepares natives/legacy asset mappings.
    /// No downloads or game process creation occur. Prepared files and directories
    /// remain on disk, including when a later preparation step fails.
    #[tracing::instrument(name = "launch.prepare", skip_all, fields(version = ?self.version.name()), err)]
    pub fn prepare(self) -> Result<PreparedLaunch, PreparationError> {
        tracing::info!("Preparing local launch files");
        if let Some(options) = self.options.verification {
            tracing::info!(?options, "Verifying selected resources");
            let report = self
                .directory
                .check(&self.version, options, &self.options.rules);
            if !report.is_intact() {
                return Err(PreparationError::Verification {
                    report: Box::new(report),
                });
            }
        }
        tracing::info!(
            files = self.required.len(),
            "Checking required launch files"
        );
        for path in &self.required {
            tracing::trace!(?path, "Checking required launch file");
            require_file(path)?;
        }
        for path in [&self.game, &self.working] {
            tracing::debug!(?path, "Preparing launch directory");
            fs::create_dir_all(path)
                .map_err(|source| io_error("create launch directory", path, source))?;
        }
        check_java(&self)?;
        if self.options.natives.extract {
            tracing::debug!(path = ?self.natives, "Preparing natives directory");
            safe_directory(&self.natives)?;
        } else {
            tracing::info!(path = ?self.natives, "Using existing natives without extraction");
            let metadata = fs::metadata(&self.natives)
                .map_err(|source| io_error("inspect existing natives", &self.natives, source))?;
            if !metadata.is_dir() {
                return Err(PreparationError::UnsafeDestination {
                    path: self.natives.clone(),
                });
            }
        }
        if self.options.natives.extract {
            extract_natives(&self)?;
        }
        if !self.mappings.is_empty() {
            tracing::info!(
                files = self.mappings.len(),
                "Preparing legacy asset mappings"
            );
        }
        for (source, destination) in &self.mappings {
            tracing::trace!(?source, ?destination, "Mapping legacy asset");
            copy_asset(source, destination)?;
        }
        tracing::info!("Launch preparation complete");
        Ok(PreparedLaunch { plan: self })
    }
}

impl PreparedLaunch {
    pub fn plan(&self) -> &LaunchPlan {
        &self.plan
    }
}

fn require_file(path: &Path) -> Result<(), PreparationError> {
    let metadata = fs::metadata(path)
        .map_err(|source| io_error("inspect required launch file", path, source))?;
    if !metadata.is_file() {
        return Err(PreparationError::NotFile { path: path.into() });
    }
    Ok(())
}

fn extract_natives(plan: &LaunchPlan) -> Result<(), PreparationError> {
    let mut extracted = 0_u64;
    let maximum = plan.options.natives.maximum_bytes;
    tracing::info!(archives = plan.archives.len(), destination = ?plan.natives, "Preparing native extraction");
    for archive in &plan.archives {
        tracing::info!(archive = ?archive.path, destination = ?plan.natives, "Extracting native archive");
        let file = fs::File::open(&archive.path)
            .map_err(|source| io_error("open native archive", &archive.path, source))?;
        let mut zip = ZipArchive::new(file).map_err(|source| PreparationError::Archive {
            path: archive.path.clone(),
            source,
        })?;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|source| PreparationError::Archive {
                    path: archive.path.clone(),
                    source,
                })?;
            let name = entry
                .name()
                .map_err(|source| PreparationError::Archive {
                    path: archive.path.clone(),
                    source,
                })?
                .into_owned();
            if archive
                .excludes
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                tracing::trace!(entry = ?name, "Skipping excluded native entry");
                continue;
            }
            let unsafe_entry = || PreparationError::UnsafeEntry {
                archive: archive.path.clone(),
                entry: name.clone(),
            };
            // Reject traversal, absolute/Windows drive paths, and archive symlinks.
            let destination =
                super::super::path::within(&plan.natives, &name).map_err(|_| unsafe_entry())?;
            let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
            if !matches!(kind, 0 | 0o100000 | 0o040000) {
                return Err(unsafe_entry());
            }
            if entry.is_dir() {
                safe_directory(&destination)?;
                continue;
            }
            tracing::trace!(entry = ?name, ?destination, bytes = entry.size(), "Extracting native file");
            if entry.size() > maximum.saturating_sub(extracted) {
                return Err(PreparationError::ExtractionLimit {
                    maximum_bytes: maximum,
                });
            }
            let parent = destination.parent().ok_or_else(unsafe_entry)?;
            safe_directory(parent)?;
            let mut output = tempfile::NamedTempFile::new_in(parent)
                .map_err(|source| io_error("create native staging file", parent, source))?;
            let copied = io::copy(
                &mut (&mut entry).take(maximum.saturating_sub(extracted).saturating_add(1)),
                &mut output,
            )
            .map_err(|source| io_error("extract native", &destination, source))?;
            extracted = extracted.saturating_add(copied);
            if extracted > maximum {
                return Err(PreparationError::ExtractionLimit {
                    maximum_bytes: maximum,
                });
            }
            output
                .persist(&destination)
                .map_err(|error| io_error("install native", &destination, error.error))?;
        }
    }
    tracing::info!(bytes = extracted, "Native extraction complete");
    Ok(())
}

fn copy_asset(source: &Path, destination: &Path) -> Result<(), PreparationError> {
    let parent = destination
        .parent()
        .expect("asset destination has a parent");
    safe_directory(parent)?;
    let mut staging = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| io_error("create asset staging file", parent, source))?;
    let mut input =
        fs::File::open(source).map_err(|error| io_error("read mapped asset", source, error))?;
    io::copy(&mut input, &mut staging)
        .map_err(|source| io_error("copy mapped asset", destination, source))?;
    staging
        .persist(destination)
        .map_err(|error| io_error("install mapped asset", destination, error.error))?;
    Ok(())
}

/// Walk from the closest existing ancestor, rejecting symlinks in the portion
/// being written. Existing canonical prefixes (e.g. macOS /var -> /private/var)
/// are resolved by the caller before this check.
fn safe_directory(path: &Path) -> Result<(), PreparationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(PreparationError::UnsafeDestination { path: path.into() })
        }
        Ok(_) => {
            // Also inspect existing intermediate directories, not just the leaf.
            for parent in path.ancestors().skip(1) {
                if parent.as_os_str().is_empty() {
                    continue;
                }
                let metadata = fs::symlink_metadata(parent)
                    .map_err(|source| io_error("inspect destination parent", parent, source))?;
                if metadata.file_type().is_symlink() {
                    return Err(PreparationError::UnsafeDestination {
                        path: parent.into(),
                    });
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                safe_directory(parent)?;
            }
            fs::create_dir(path)
                .or_else(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|source| io_error("create destination directory", path, source))?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|source| io_error("inspect destination directory", path, source))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(PreparationError::UnsafeDestination { path: path.into() });
            }
            Ok(())
        }
        Err(source) => Err(io_error("inspect destination directory", path, source)),
    }
}

fn check_java(plan: &LaunchPlan) -> Result<(), PreparationError> {
    if plan.options.java.version_check == JavaVersionCheck::Skip {
        tracing::debug!("Java version check disabled");
        return Ok(());
    }
    let Some(required) = plan.java_major else {
        tracing::debug!("No manifest Java version requirement");
        return Ok(());
    };
    tracing::info!(java = ?plan.executable, required, policy = ?plan.options.java.version_check, "Checking Java version");
    let mut command = Command::new(&plan.executable);
    super::process::configure_environment(&mut command, &plan.options.process);
    let output = command
        .arg("-version")
        .current_dir(&plan.working)
        .output()
        .map_err(|source| io_error("probe Java version", &plan.executable, source))?;
    if !output.status.success() {
        return Err(PreparationError::JavaVersionUnknown);
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let actual = text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if !(line.starts_with("java version")
                || line.starts_with("openjdk version")
                || line.starts_with("openjdk ")
                || line.starts_with("java "))
            {
                return None;
            }
            let version = line
                .split_whitespace()
                .find(|part| {
                    part.trim_start_matches('"')
                        .starts_with(|c: char| c.is_ascii_digit())
                })?
                .trim_matches('"');
            let mut numbers = version.split(['.', '-', '+', '_']);
            let first = numbers.next()?.parse::<u32>().ok()?;
            if first == 1 {
                numbers.next()?.parse().ok()
            } else {
                Some(first)
            }
        })
        .ok_or(PreparationError::JavaVersionUnknown)?;
    let policy = plan.options.java.version_check;
    tracing::debug!(actual, required, ?policy, "Resolved Java major version");
    if actual < required || (policy == JavaVersionCheck::ExactManifest && actual != required) {
        return Err(PreparationError::JavaVersionMismatch {
            actual,
            required,
            policy,
        });
    }
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> PreparationError {
    PreparationError::Io {
        operation,
        path: path.into(),
        source,
    }
}
