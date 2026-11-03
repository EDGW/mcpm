//! Controlled Java argument-file transport probe, without user arguments or agents.

use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Tests the transport mcpm needs for oversized commands, without validating JVM tuning.
/// The probe's argument file contains only `-version`; Java 8 cannot expand it.
pub(crate) fn argument_files(
    executable: &Path,
    directory: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<bool> {
    let mut argument_file = tempfile::NamedTempFile::new()?;
    writeln!(argument_file, "-version")?;
    argument_file.flush()?;
    let mut argument = OsString::from("@");
    argument.push(argument_file.path());
    let mut stdout = tempfile::tempfile().context("failed to prepare Java capability output")?;
    let mut stderr = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    command.current_dir(directory).env_clear().envs(environment);
    for name in ["JDK_JAVA_OPTIONS", "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS"] {
        command.env_remove(name);
    }
    let mut child = command
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .with_context(|| format!("failed to inspect Java capabilities using {executable:?}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(15) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            result => {
                let _ = child.kill();
                child
                    .wait()
                    .context("failed to reap Java capability probe")?;
                result.context("failed to wait for Java capability probe")?;
                anyhow::bail!("Java capability probe timed out");
            }
        }
    };
    bounded_output(&mut stderr)?;
    bounded_output(&mut stdout)?;
    Ok(status.success())
}

fn bounded_output(file: &mut File) -> Result<String> {
    use std::io::{Seek, SeekFrom};
    ensure!(
        file.metadata()?.len() <= 8 * 1024 * 1024,
        "Java capability output exceeded 8 MiB"
    );
    file.seek(SeekFrom::Start(0))?;
    let mut result = String::new();
    file.read_to_string(&mut result)?;
    Ok(result)
}
