//! Isolated staging workspace and Java installer subprocess execution.
//!
//! Creates `<server>/.mcpm/loader-install/<id>/` with a mirrored `stage/<workingDirectory>/`
//! tree, copies (never hard-links) inputs into the staging workspace, runs the installer JAR
//! using [`crate::local::java::executable`] and [`crate::local::java::process`], and removes
//! the staging workspace on completion or drop.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::ExitStatus,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    download::MappingTable,
    local::{
        java::{
            executable,
            process::{ManagedProcess, ProcessSpec},
        },
        mc::server::config::{LaunchConfig, ServerConfig},
    },
};

const MAX_LOG_TAIL_BYTES: usize = 16 * 1024;
const MAX_DEBUG_LINE_BYTES: usize = 4096;
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Returned when loader installation is cancelled before entering the publication window.
#[derive(Debug, Error)]
#[error("loader installation cancelled")]
pub struct InstallCancelled;

/// Phase 4 mirror-agent hook reserved for installer HTTP rewriting.
///
/// When `table.is_empty()` (e.g. `OfficialSource`), no `-javaagent` is attached.
#[derive(Debug, Clone)]
pub struct MirrorAgentSpec {
    pub agent_jar: PathBuf,
    pub table: MappingTable,
    pub report_path: PathBuf,
}

/// Owned temporary workspace under `<server>/.mcpm/loader-install/<id>/`.
///
/// Dropping without calling [`Self::cleanup`] automatically attempts to remove the directory
/// so pre-commit failures and cancellations leave no stale staging files behind.
pub struct StagingDir {
    base_dir: PathBuf,
    installer_jar: PathBuf,
    reports_dir: PathBuf,
    stage_root: PathBuf,
    stage_cwd: PathBuf,
    cleaned: bool,
}

impl StagingDir {
    /// Creates a new isolated staging directory mirroring `working_directory_rel` under `stage/`.
    pub fn create(server_root: &Path, working_directory_rel: &Path) -> Result<Self> {
        ensure!(
            server_root.is_dir(),
            "server root is not an existing directory: {server_root:?}"
        );
        let canonical_root = std::fs::canonicalize(server_root)
            .with_context(|| format!("failed to resolve server root {server_root:?}"))?;

        let mcpm_dir = canonical_root.join(".mcpm");
        ensure_regular_directory(&mcpm_dir, "server metadata directory")?;

        let staging_parent = mcpm_dir.join("loader-install");
        ensure_regular_directory(&staging_parent, "loader staging root")?;

        let id = Uuid::new_v4().to_string();
        let base_dir = staging_parent.join(&id);
        let reports_dir = base_dir.join("reports");
        let stage_root = base_dir.join("stage");
        let stage_cwd = if working_directory_rel.as_os_str().is_empty() {
            stage_root.clone()
        } else {
            ensure_clean_relative(working_directory_rel, "working directory")?;
            stage_root.join(working_directory_rel)
        };

        fs::create_dir(&base_dir)
            .with_context(|| format!("failed to create loader staging directory {base_dir:?}"))?;
        let init = (|| -> Result<()> {
            fs::create_dir(&reports_dir)
                .with_context(|| format!("failed to create staging reports dir {reports_dir:?}"))?;
            fs::create_dir_all(&stage_cwd)
                .with_context(|| format!("failed to create staging cwd {stage_cwd:?}"))?;
            Ok(())
        })();
        if let Err(error) = init {
            let _ = fs::remove_dir_all(&base_dir);
            return Err(error);
        }

        let installer_jar = base_dir.join("installer.jar");
        Ok(Self {
            base_dir,
            installer_jar,
            reports_dir,
            stage_root,
            stage_cwd,
            cleaned: false,
        })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn installer_jar_path(&self) -> &Path {
        &self.installer_jar
    }

    pub fn reports_dir(&self) -> &Path {
        &self.reports_dir
    }

    pub fn stage_root(&self) -> &Path {
        &self.stage_root
    }

    pub fn stage_cwd(&self) -> &Path {
        &self.stage_cwd
    }

    /// Copies a file into `stage_cwd/<rel_in_cwd>` (never hard-links real server files).
    pub fn copy_into_cwd(&self, source: &Path, rel_in_cwd: &Path) -> Result<PathBuf> {
        ensure_clean_relative(rel_in_cwd, "staged cwd file")?;
        let destination = self.stage_cwd.join(rel_in_cwd);
        copy_file_checked(source, &destination)?;
        Ok(destination)
    }

    /// Copies a file into `stage_root/<rel_in_stage>` (used for in-root verification copies).
    pub fn copy_into_stage_root(&self, source: &Path, rel_in_stage: &Path) -> Result<PathBuf> {
        ensure_clean_relative(rel_in_stage, "staged server file")?;
        let destination = self.stage_root.join(rel_in_stage);
        copy_file_checked(source, &destination)?;
        Ok(destination)
    }

    /// Writes raw bytes into `stage_cwd/<rel_in_cwd>`, creating parent directories as needed.
    pub fn write_into_cwd(&self, rel_in_cwd: &Path, bytes: &[u8]) -> Result<PathBuf> {
        ensure_clean_relative(rel_in_cwd, "staged cwd file")?;
        let destination = self.stage_cwd.join(rel_in_cwd);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create staging parent {parent:?}"))?;
        }
        fs::write(&destination, bytes)
            .with_context(|| format!("failed to write staged file {destination:?}"))?;
        Ok(destination)
    }

    /// Removes the staging workspace immediately.
    pub fn cleanup(mut self) -> Result<()> {
        self.cleaned = true;
        remove_staging_dir(&self.base_dir)
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(error) = remove_staging_dir(&self.base_dir)
        {
            tracing::warn!(
                path = %self.base_dir.display(),
                %error,
                "failed to clean up loader staging directory on drop"
            );
        }
    }
}

/// Captured installer subprocess result and log artifacts under `reports/`.
#[derive(Debug, Clone)]
pub struct InstallerExecution {
    pub status: ExitStatus,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

/// Runs `staging.installer_jar_path()` inside `staging.stage_cwd()` without server JVM
/// tuning or `authlib-injector`, capturing stdout/stderr into `reports/`.
///
/// When `cancel` becomes `true`, the installer process tree is immediately terminated and
/// reaped via [`ManagedProcess::kill`] and [`InstallCancelled`] is returned.
pub fn run_installer(
    server_root: &Path,
    launch: &LaunchConfig,
    conf: &ServerConfig,
    staging: &StagingDir,
    installer_args: &[String],
    agent: Option<&MirrorAgentSpec>,
    round: usize,
    cancel: Option<&AtomicBool>,
) -> Result<InstallerExecution> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(InstallCancelled.into());
    }
    let environment = executable::merge_environment(&launch.process.environment);
    let java = executable::resolve(
        server_root,
        conf.java.executable.as_deref().unwrap_or("java"),
        &environment,
    )?;

    let mut arguments = Vec::new();
    if let Some(spec) = agent
        && !spec.table.is_empty()
    {
        bail!(
            "mirror agent injection ({:?}) requires Phase 4 mcloader-injector integration",
            spec.agent_jar
        );
    }

    let installer_jar = staging
        .installer_jar_path()
        .to_str()
        .context("installer JAR path is not valid UTF-8")?;
    arguments.push("-jar".to_owned());
    arguments.push(installer_jar.to_owned());
    arguments.extend(installer_args.iter().cloned());

    tracing::debug!(
        round,
        executable = %java.display(),
        cwd = %staging.stage_cwd().display(),
        ?arguments,
        "starting loader installer"
    );

    let mut process = ManagedProcess::spawn(ProcessSpec {
        executable: &java,
        directory: staging.stage_cwd(),
        environment: &environment,
        arguments: &arguments,
        argument_file: false,
    })?;

    // Close stdin immediately so interactive prompts in legacy installers cannot hang.
    drop(process.take_stdin());
    let stdout = process.take_stdout();
    let stderr = process.take_stderr();

    let stdout_log = staging
        .reports_dir()
        .join(format!("round-{round}-stdout.log"));
    let stderr_log = staging
        .reports_dir()
        .join(format!("round-{round}-stderr.log"));

    let parent_span = tracing::Span::current();
    let out_path = stdout_log.as_path();
    let err_path = stderr_log.as_path();
    let (stdout_tail, stderr_tail, status) = thread::scope(|scope| -> Result<_> {
        let out_span = parent_span.clone();
        let err_span = parent_span.clone();
        let out_handle = scope.spawn(move || {
            let _entered = out_span.enter();
            drain_pipe(stdout, out_path, "stdout", round)
        });
        let err_handle = scope.spawn(move || {
            let _entered = err_span.enter();
            drain_pipe(stderr, err_path, "stderr", round)
        });
        let wait_result = wait_with_cancel(&mut process, cancel);
        let out_result = out_handle
            .join()
            .map_err(|_| anyhow::anyhow!("installer stdout reader thread panicked"))?;
        let err_result = err_handle
            .join()
            .map_err(|_| anyhow::anyhow!("installer stderr reader thread panicked"))?;
        let status = wait_result?;
        Ok((out_result?, err_result?, status))
    })?;

    tracing::debug!(round, %status, "loader installer exited");

    Ok(InstallerExecution {
        status,
        stdout_log,
        stderr_log,
        stdout_tail,
        stderr_tail,
    })
}

fn wait_with_cancel(
    process: &mut ManagedProcess,
    cancel: Option<&AtomicBool>,
) -> Result<ExitStatus> {
    let Some(flag) = cancel else {
        return process.wait();
    };
    loop {
        if let Some(status) = process.try_wait()? {
            return Ok(status);
        }
        if flag.load(Ordering::Relaxed) {
            let _ = process.kill();
            let _ = process.wait();
            return Err(InstallCancelled.into());
        }
        thread::sleep(CANCEL_POLL_INTERVAL);
    }
}

fn drain_pipe(
    pipe: Option<impl Read>,
    log_path: &Path,
    stream: &'static str,
    round: usize,
) -> Result<String> {
    let mut log = File::create(log_path)
        .with_context(|| format!("failed to create installer log {log_path:?}"))?;
    let Some(mut pipe) = pipe else {
        return Ok(String::new());
    };
    let mut tail = Vec::new();
    let mut pending_line = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = pipe
            .read(&mut buffer)
            .with_context(|| format!("failed to read installer output for {log_path:?}"))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        log.write_all(chunk)
            .with_context(|| format!("failed to write installer log {log_path:?}"))?;
        append_tail(&mut tail, chunk);
        if tracing::enabled!(tracing::Level::DEBUG) {
            log_installer_chunk(&mut pending_line, chunk, stream, round);
        }
    }
    if !pending_line.is_empty() && tracing::enabled!(tracing::Level::DEBUG) {
        emit_installer_line(&pending_line, stream, round);
    }
    log.flush()
        .with_context(|| format!("failed to flush installer log {log_path:?}"))?;
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

fn log_installer_chunk(pending: &mut Vec<u8>, chunk: &[u8], stream: &'static str, round: usize) {
    for &byte in chunk {
        if byte == b'\n' || byte == b'\r' {
            if !pending.is_empty() {
                emit_installer_line(pending, stream, round);
                pending.clear();
            }
        } else {
            pending.push(byte);
            if pending.len() >= MAX_DEBUG_LINE_BYTES {
                emit_installer_line(pending, stream, round);
                pending.clear();
            }
        }
    }
}

fn emit_installer_line(line: &[u8], stream: &'static str, round: usize) {
    let text = String::from_utf8_lossy(line);
    let trimmed = text.trim_end();
    if !trimmed.is_empty() {
        tracing::debug!(round, stream, "{trimmed}");
    }
}

fn append_tail(tail: &mut Vec<u8>, chunk: &[u8]) {
    if chunk.len() >= MAX_LOG_TAIL_BYTES {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - MAX_LOG_TAIL_BYTES..]);
    } else {
        let overflow = (tail.len() + chunk.len()).saturating_sub(MAX_LOG_TAIL_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(chunk);
    }
}

fn copy_file_checked(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create staging parent {parent:?}"))?;
    }
    fs::copy(source, destination)
        .with_context(|| format!("failed to copy {source:?} into staging path {destination:?}"))?;
    Ok(())
}

fn ensure_regular_directory(path: &Path, label: &str) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).with_context(|| format!("failed to create {label} {path:?}")),
    }
    let meta = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {label} {path:?}"))?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "{label} must be a regular directory (not a symlink): {path:?}"
    );
    Ok(())
}

pub(crate) fn ensure_clean_relative(path: &Path, label: &str) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty(),
        "{label} path cannot be empty"
    );
    ensure!(
        !path.is_absolute(),
        "{label} path must be relative: {path:?}"
    );
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => bail!("{label} path cannot escape its root: {path:?}"),
        }
    }
    Ok(())
}

fn remove_staging_dir(base_dir: &Path) -> Result<()> {
    match fs::remove_dir_all(base_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to remove staging directory {base_dir:?}"));
        }
    }
    if let Some(parent) = base_dir.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::{sync::Arc, time::Instant};

    use super::*;
    use crate::local::mc::server::config;

    #[test]
    fn cancel_flag_kills_installer_process_tree_and_drop_cleans_staging() {
        let dir = tempfile::tempdir().unwrap();
        let staging_base = {
            let staging = StagingDir::create(dir.path(), Path::new("")).unwrap();
            let base = staging.base_dir().to_path_buf();
            // Fake executable that ignores `-jar <path>` and sleeps 30s so cancel must kill it.
            let script = dir.path().join("fake-java.sh");
            fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            }

            let draft = config::vanilla_server().unwrap();
            let mut conf = config::decode_conf(&draft.template().unwrap()).unwrap();
            conf.java.executable = Some(script.to_str().unwrap().to_owned());

            let cancel = Arc::new(AtomicBool::new(false));
            let cancel_trigger = cancel.clone();
            let trigger = thread::spawn(move || {
                thread::sleep(Duration::from_millis(120));
                cancel_trigger.store(true, Ordering::Relaxed);
            });

            let start = Instant::now();
            let err = run_installer(
                dir.path(),
                &draft.launch,
                &conf,
                &staging,
                &[],
                None,
                1,
                Some(cancel.as_ref()),
            )
            .unwrap_err();
            trigger.join().unwrap();

            assert!(
                err.downcast_ref::<InstallCancelled>().is_some(),
                "expected InstallCancelled, got: {err:#}"
            );
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "cancellation should kill installer promptly, took {:?}",
                start.elapsed()
            );
            base
        };
        assert!(
            !staging_base.exists(),
            "dropping StagingDir after cancellation must clean up {staging_base:?}"
        );
    }
}
