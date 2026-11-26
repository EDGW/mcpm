//! Allowlist-based staged artifact publication and atomic v1 metadata/launch commit.
//!
//! Runs exclusively inside a caller-held write [`ServerLock`]: re-verifies the
//! [`PreflightSnapshot`], materializes the adapter's [`PublishPlan`] allowlist (skipping
//! `unix_args.txt` and `win_args.txt` under `libraries/`), performs a complete pre-write
//! symlink-boundary, conflict, and vanilla-JAR protection pass before modifying any file,
//! publishes each file atomically, and commits `.mcpm/server.json` + `launch.toml` through
//! [`config::commit`] while leaving `conf.toml` untouched.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::{
    preflight::{self, PreflightSnapshot},
    runner::{self, StagingDir},
    verify::VerifiedInstall,
};
use crate::local::mc::server::config::{
    self, ConfAction, LaunchAction, Publication, Published, ServerLock,
};

const COMPARE_BUFFER_BYTES: usize = 8192;

/// Result of publishing staged loader artifacts and atomically committing server metadata.
#[derive(Debug)]
pub struct PublishOutcome {
    pub published: Published,
    pub published_files: Vec<PathBuf>,
    pub skipped_identical_files: Vec<PathBuf>,
}

/// Re-verifies `snapshot`, checks every allowlisted target before writing, publishes
/// staged runtime files into `root`, and atomically commits `.mcpm/server.json` and
/// `launch.toml` under the caller's exclusive [`ServerLock`].
pub fn publish_locked(
    root: &Path,
    lock: &ServerLock,
    snapshot: &PreflightSnapshot,
    verified: &VerifiedInstall,
    staging: &StagingDir,
    force: bool,
) -> Result<PublishOutcome> {
    let canonical_root = std::fs::canonicalize(root)
        .with_context(|| format!("failed to resolve server root {root:?}"))?;
    ensure!(
        canonical_root == snapshot.root,
        "publish root {canonical_root:?} does not match preflight snapshot root {:?}",
        snapshot.root
    );

    lock.ensure_write_for(&snapshot.root, "publishing loader installation")?;
    preflight::verify_snapshot_fresh(&snapshot.root, lock, snapshot)?;

    let relative_files = collect_manifest_files(staging.stage_cwd(), &verified.publish_plan)?;
    let planned = plan_writes(
        snapshot,
        staging.stage_cwd(),
        &relative_files,
        &verified.publish_plan.merged_files,
        force,
    )?;

    let mut published_files = Vec::new();
    let mut created_files = Vec::new();
    let mut created_dirs = Vec::new();
    let mut skipped_identical_files = Vec::new();
    let write_and_commit = (|| -> Result<Published> {
        for entry in planned {
            match entry.action {
                FileAction::SkipIdentical => {
                    skipped_identical_files.push(entry.destination);
                }
                FileAction::Create => {
                    ensure_parent_directories(
                        &snapshot.root,
                        &entry.rel_in_server,
                        &mut created_dirs,
                    )?;
                    write_file_atomically(&entry.source, &entry.destination, false)?;
                    created_files.push(entry.destination.clone());
                    published_files.push(entry.destination);
                }
                FileAction::Replace => {
                    ensure_parent_directories(
                        &snapshot.root,
                        &entry.rel_in_server,
                        &mut created_dirs,
                    )?;
                    write_file_atomically(&entry.source, &entry.destination, true)?;
                    published_files.push(entry.destination);
                }
            }
        }

        config::commit(
            &snapshot.root,
            lock,
            Publication {
                identity: &verified.metadata,
                launch: LaunchAction::Write(&verified.launch),
                conf: ConfAction::Keep,
                replace_existing: true,
                recover_pending: false,
            },
        )
    })();

    let published = match write_and_commit {
        Ok(published) => published,
        Err(error) => {
            cleanup_created_entries(&snapshot.root, &created_files, &created_dirs);
            return Err(error);
        }
    };

    Ok(PublishOutcome {
        published,
        published_files,
        skipped_identical_files,
    })
}

struct PlannedFile {
    source: PathBuf,
    rel_in_server: PathBuf,
    destination: PathBuf,
    action: FileAction,
}

enum FileAction {
    Create,
    Replace,
    SkipIdentical,
}

fn collect_manifest_files(
    stage_cwd: &Path,
    plan: &super::adapters::PublishPlan,
) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();

    for rel in &plan.explicit_files {
        runner::ensure_clean_relative(rel, "publish plan file")?;
        let full = stage_cwd.join(rel);
        let meta = fs::symlink_metadata(&full)
            .with_context(|| format!("staged file {full:?} disappeared before publish"))?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "staged file {full:?} must be a regular file"
        );
        files.insert(rel.clone());
    }

    if plan.include_libraries {
        let libs_root = stage_cwd.join("libraries");
        match fs::symlink_metadata(&libs_root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect staged {libs_root:?}"));
            }
            Ok(meta) => {
                ensure!(
                    meta.is_dir() && !meta.file_type().is_symlink(),
                    "staged libraries path {libs_root:?} must be a regular directory"
                );
                collect_libraries_recursive(stage_cwd, Path::new("libraries"), &mut files)?;
            }
        }
    }

    Ok(files.into_iter().collect())
}

fn collect_libraries_recursive(
    stage_cwd: &Path,
    rel_dir: &Path,
    out: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let dir_path = stage_cwd.join(rel_dir);
    let mut entries: Vec<_> = fs::read_dir(&dir_path)
        .with_context(|| format!("failed to read staged directory {dir_path:?}"))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to enumerate staged directory {dir_path:?}"))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry.file_name();
        let rel = rel_dir.join(&name);
        let full = stage_cwd.join(&rel);
        let meta = fs::symlink_metadata(&full)
            .with_context(|| format!("failed to inspect staged path {full:?}"))?;
        ensure!(
            !meta.file_type().is_symlink(),
            "refusing to publish symlink from staged libraries: {full:?}"
        );
        if meta.is_dir() {
            collect_libraries_recursive(stage_cwd, &rel, out)?;
        } else if meta.is_file() {
            let name_str = name.to_string_lossy();
            if name_str.eq_ignore_ascii_case("unix_args.txt")
                || name_str.eq_ignore_ascii_case("win_args.txt")
            {
                continue;
            }
            out.insert(rel);
        } else {
            bail!("refusing to publish non-regular staged file: {full:?}");
        }
    }
    Ok(())
}

fn plan_writes(
    snapshot: &PreflightSnapshot,
    stage_cwd: &Path,
    relative_files: &[PathBuf],
    merged_files: &BTreeMap<PathBuf, String>,
    force: bool,
) -> Result<Vec<PlannedFile>> {
    if !snapshot.working_directory_is_root() {
        check_existing_ancestors_regular(&snapshot.root, &snapshot.working_directory_rel, true)?;
    }

    let mut planned = Vec::with_capacity(relative_files.len());
    for rel_in_cwd in relative_files {
        let source = stage_cwd.join(rel_in_cwd);
        let rel_in_server = if snapshot.working_directory_is_root() {
            rel_in_cwd.clone()
        } else {
            snapshot.working_directory_rel.join(rel_in_cwd)
        };
        runner::ensure_clean_relative(&rel_in_server, "server publish target")?;
        ensure_not_control_path(&rel_in_server)?;

        if let Some(parent_rel) = rel_in_server.parent()
            && !parent_rel.as_os_str().is_empty()
        {
            check_existing_ancestors_regular(&snapshot.root, parent_rel, false)?;
        }

        let destination = snapshot.root.join(&rel_in_server);
        let expected_merged_hash = merged_files.get(rel_in_cwd);
        let action = match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ensure!(
                    expected_merged_hash.is_none(),
                    "merged target file {destination:?} was removed during loader installation; retry the command"
                );
                FileAction::Create
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect target path {destination:?}"));
            }
            Ok(meta) => {
                ensure!(
                    !meta.file_type().is_symlink() && meta.is_file(),
                    "refusing to overwrite non-regular or symlink path {destination:?}"
                );
                let identical = files_byte_identical(&source, &destination)?;
                let canonical_dest = std::fs::canonicalize(&destination)
                    .with_context(|| format!("failed to resolve target path {destination:?}"))?;
                if canonical_dest == snapshot.vanilla_jar_path {
                    ensure!(
                        identical,
                        "refusing to overwrite original vanilla server JAR {destination:?} (even with --force)"
                    );
                    FileAction::SkipIdentical
                } else if let Some(expected_hash) = expected_merged_hash {
                    let current_hash = preflight::hash_file(&destination).with_context(|| {
                        format!("failed to re-hash merged target file {destination:?}")
                    })?;
                    ensure!(
                        current_hash == *expected_hash,
                        "merged target file {destination:?} changed during loader installation; retry the command"
                    );
                    if identical {
                        FileAction::SkipIdentical
                    } else {
                        FileAction::Replace
                    }
                } else if identical {
                    FileAction::SkipIdentical
                } else if force {
                    FileAction::Replace
                } else {
                    bail!(
                        "target file {destination:?} already exists with different content; pass --force to replace it"
                    );
                }
            }
        };

        planned.push(PlannedFile {
            source,
            rel_in_server,
            destination,
            action,
        });
    }
    Ok(planned)
}

fn ensure_not_control_path(rel_in_server: &Path) -> Result<()> {
    let parts: Vec<&str> = rel_in_server
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    if let Some(first) = parts.first() {
        ensure!(
            !first.eq_ignore_ascii_case(".mcpm"),
            "refusing to publish loader artifact into protected .mcpm directory: {rel_in_server:?}"
        );
        if parts.len() == 1 {
            ensure!(
                !first.eq_ignore_ascii_case("launch.toml")
                    && !first.eq_ignore_ascii_case("conf.toml"),
                "refusing to publish loader artifact over protected mcpm configuration file: {rel_in_server:?}"
            );
        }
    }
    Ok(())
}

fn check_existing_ancestors_regular(
    root: &Path,
    rel_dir: &Path,
    must_exist: bool,
) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in rel_dir.components() {
        match component {
            Component::Normal(part) => {
                current.push(part);
                match fs::symlink_metadata(&current) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        ensure!(
                            !must_exist,
                            "expected server directory {current:?} no longer exists"
                        );
                        break;
                    }
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("failed to inspect directory {current:?}"));
                    }
                    Ok(meta) => {
                        ensure!(
                            !meta.file_type().is_symlink() && meta.is_dir(),
                            "refusing to traverse symlink or non-directory ancestor {current:?}"
                        );
                    }
                }
            }
            Component::CurDir => {}
            _ => bail!("invalid relative publish directory {rel_dir:?}"),
        }
    }
    Ok(())
}

fn ensure_parent_directories(
    root: &Path,
    rel_file: &Path,
    created_dirs: &mut Vec<PathBuf>,
) -> Result<()> {
    let Some(parent_rel) = rel_file.parent() else {
        return Ok(());
    };
    let mut current = root.to_path_buf();
    for component in parent_rel.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            match fs::create_dir(&current) {
                Ok(()) => created_dirs.push(current.clone()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to create directory {current:?}"));
                }
            }
            let meta = fs::symlink_metadata(&current)
                .with_context(|| format!("failed to inspect directory {current:?}"))?;
            ensure!(
                !meta.file_type().is_symlink() && meta.is_dir(),
                "refusing to write through symlink or non-directory {current:?}"
            );
        }
    }
    Ok(())
}

fn files_byte_identical(a: &Path, b: &Path) -> Result<bool> {
    let meta_a = fs::metadata(a).with_context(|| format!("failed to stat {a:?}"))?;
    let meta_b = fs::metadata(b).with_context(|| format!("failed to stat {b:?}"))?;
    if meta_a.len() != meta_b.len() {
        return Ok(false);
    }
    let mut file_a = File::open(a).with_context(|| format!("failed to open {a:?}"))?;
    let mut file_b = File::open(b).with_context(|| format!("failed to open {b:?}"))?;
    let mut buf_a = [0u8; COMPARE_BUFFER_BYTES];
    let mut buf_b = [0u8; COMPARE_BUFFER_BYTES];
    loop {
        let n_a = read_full_chunk(&mut file_a, &mut buf_a)
            .with_context(|| format!("failed to read {a:?}"))?;
        let n_b = read_full_chunk(&mut file_b, &mut buf_b)
            .with_context(|| format!("failed to read {b:?}"))?;
        if n_a != n_b || buf_a[..n_a] != buf_b[..n_b] {
            return Ok(false);
        }
        if n_a == 0 {
            return Ok(true);
        }
    }
}

fn read_full_chunk(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}

fn write_file_atomically(source: &Path, destination: &Path, allow_replace: bool) -> Result<()> {
    let parent = destination
        .parent()
        .context("publish target has no parent directory")?;
    let mut src = File::open(source)
        .with_context(|| format!("failed to open staged artifact {source:?}"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {parent:?}"))?;
    io::copy(&mut src, &mut temp)
        .with_context(|| format!("failed to copy staged artifact {source:?} to {destination:?}"))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("failed to flush temporary file for {destination:?}"))?;
    let persist_result = if allow_replace {
        temp.persist(destination).map_err(|error| error.error)
    } else {
        temp.persist_noclobber(destination)
            .map_err(|error| error.error)
    };
    persist_result.with_context(|| format!("failed to publish artifact {destination:?}"))?;
    Ok(())
}

/// Best-effort cleanup of brand-new files and directories created during an interrupted
/// publish pass before `config::commit` succeeded; never removes pre-existing entries.
fn cleanup_created_entries(root: &Path, created_files: &[PathBuf], created_dirs: &[PathBuf]) {
    for path in created_files.iter().rev() {
        let _ = fs::remove_file(path);
    }
    for dir in created_dirs.iter().rev() {
        if dir != root && dir.starts_with(root) {
            let _ = fs::remove_dir(dir);
        }
    }
}
