//! Java process construction and platform stop escalation.

use std::{
    io::Write,
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result};

use super::StartSpec;

pub struct JavaChild {
    pub child: Child,
    _arguments: Option<tempfile::NamedTempFile>,
}

pub fn spawn(spec: &StartSpec) -> Result<JavaChild> {
    let mut command = Command::new(&spec.executable);
    command
        .current_dir(&spec.directory)
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let arguments = if spec.argument_file {
        let mut file = tempfile::NamedTempFile::new_in(&spec.directory)
            .context("failed to create Java argument file")?;
        for argument in &spec.arguments {
            writeln!(file, "\"{}\"", escape(argument))?;
        }
        file.flush()?;
        let mut value = std::ffi::OsString::from("@");
        value.push(file.path());
        command.arg(value);
        Some(file)
    } else {
        command.args(&spec.arguments);
        None
    };
    configure(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("failed to start Java {:?}", spec.executable))?;
    Ok(JavaChild {
        child,
        _arguments: arguments,
    })
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

pub fn terminate(child: &mut Child, force: bool) -> Result<()> {
    if force {
        child.kill().context("failed to force Java termination")?;
        return Ok(());
    }
    #[cfg(unix)]
    unsafe {
        let result = libc::kill(-(child.id() as i32), libc::SIGTERM);
        if result != 0 {
            let _ = child.kill();
        }
    }
    #[cfg(windows)]
    {
        if force {
            child.kill().context("failed to terminate Java process")?;
        }
    }
    #[cfg(not(any(unix, windows)))]
    child.kill().context("failed to terminate Java process")?;
    Ok(())
}

#[cfg(unix)]
fn configure(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x00000200 | 0x01000000);
}

#[cfg(not(any(unix, windows)))]
fn configure(_: &mut Command) {}
