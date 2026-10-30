//! Detached process creation and byte-oriented startup handshakes.

use std::{ffi::OsStr, io::{Read, Write}, path::{Path, PathBuf}, process::{Child, Command, Stdio}};
use thiserror::Error;

/// Configures a child process intended to outlive its caller's terminal.
#[derive(Debug)]
pub struct Detached {
    command: Command,
}

impl Detached {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        let mut command = Command::new(program);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        Self { command }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self { self.command.arg(arg); self }
    pub fn args<I, S>(mut self, args: I) -> Self where I: IntoIterator<Item = S>, S: AsRef<OsStr> { self.command.args(args); self }
    pub fn current_dir(mut self, path: impl AsRef<Path>) -> Self { self.command.current_dir(path); self }

    /// Spawns with platform detachment flags and returns the startup pipes.
    pub fn spawn(mut self) -> Result<StartupChild, SpawnError> {
        configure_detachment(&mut self.command);
        let mut child = self.command.spawn().map_err(|source| SpawnError::Spawn { source })?;
        let stdin = child.stdin.take().ok_or(SpawnError::MissingPipe("stdin"))?;
        let stdout = child.stdout.take().ok_or(SpawnError::MissingPipe("stdout"))?;
        let stderr = child.stderr.take().ok_or(SpawnError::MissingPipe("stderr"))?;
        Ok(StartupChild { child, startup: StartupPipe { stdin: Some(stdin), stdout }, stderr })
    }
}

#[derive(Debug)]
pub struct StartupChild {
    pub child: Child,
    pub startup: StartupPipe,
    pub stderr: std::process::ChildStderr,
}

/// Byte stream used by a launcher to exchange one startup frame.
#[derive(Debug)]
pub struct StartupPipe {
    stdin: Option<std::process::ChildStdin>,
    stdout: std::process::ChildStdout,
}

impl StartupPipe {
    pub fn write_frame(&mut self, bytes: &[u8]) -> Result<(), SpawnError> {
        let stdin = self.stdin.as_mut().ok_or_else(|| SpawnError::Message("startup input is closed".to_owned()))?;
        stdin.write_all(bytes).map_err(|source| SpawnError::Handshake { source })?;
        stdin.flush().map_err(|source| SpawnError::Handshake { source })
    }

    pub fn read_frame(&mut self, limit: usize) -> Result<Vec<u8>, SpawnError> {
        let mut result = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let count = self.stdout.read(&mut byte).map_err(|source| SpawnError::Handshake { source })?;
            if count == 0 { break; }
            result.push(byte[0]);
            if result.len() > limit { return Err(SpawnError::TooLarge(limit)); }
            if byte[0] == b'\n' { break; }
        }
        Ok(result)
    }

    pub fn close_input(&mut self) { self.stdin.take(); }
}

#[cfg(unix)]
fn configure_detachment(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_detachment(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x00000008 | 0x00000200 | 0x01000000);
}

#[cfg(not(any(unix, windows)))]
fn configure_detachment(_: &mut Command) {}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("failed to spawn detached process: {source}")]
    Spawn { source: std::io::Error },
    #[error("detached process did not provide {0} pipe")]
    MissingPipe(&'static str),
    #[error("startup handshake failed: {source}")]
    Handshake { source: std::io::Error },
    #[error("startup handshake exceeded {0} bytes")]
    TooLarge(usize),
    #[error("startup handshake failed: {0}")]
    Message(String),
}

#[allow(dead_code)]
fn _path_owned(path: PathBuf) -> PathBuf { path }
