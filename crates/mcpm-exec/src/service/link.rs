//! Connecting to a shared link or starting one with an inherited readiness pipe.

use super::{
    Connection,
    control::{Hello, Ready, VERSION},
};
use crate::{endpoint::Endpoint, messaging::Session, operation};
use anyhow::{Context, Result, ensure};
use std::{fs::OpenOptions, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

pub(super) async fn open(connection: &Connection, idle_timeout: u64) -> Result<Session> {
    let endpoint = Endpoint::for_key(&connection.key()?)?;
    let session = match endpoint.connect().await {
        Ok(carrier) => Session::new(carrier),
        Err(error) => {
            let retryable = error
                .chain()
                .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                .any(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    )
                });
            if !retryable {
                return Err(error);
            }
            start(connection, idle_timeout, &endpoint).await?;
            Session::new(
                endpoint
                    .connect()
                    .await
                    .context("failed to connect to the ready link")?,
            )
        }
    };
    let hello = operation::call::<Hello>(&session, &())
        .await
        .context("link handshake failed")?;
    ensure!(
        hello.version == VERSION,
        "incompatible link version {}; expected {VERSION}",
        hello.version
    );
    Ok(session)
}

async fn start(connection: &Connection, idle_timeout: u64, endpoint: &Endpoint) -> Result<()> {
    let executable = daemon_executable()?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(endpoint.log_path())
        .context("failed to open link startup log")?;
    let mut command = Command::new(executable);
    command
        .arg("link")
        .arg("--connection")
        .arg(serde_json::to_string(connection)?)
        .arg("--idle-timeout")
        .arg(idle_timeout.to_string())
        .arg("--ready")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x00000200 | 0x08000000);
    let mut child = command.spawn().context("failed to start mcpmd link")?;
    let stdout = child
        .stdout
        .take()
        .context("link readiness pipe is unavailable")?;
    let mut reader = BufReader::new(stdout);
    let mut message = String::new();
    let result = reader
        .read_line(&mut message)
        .await
        .context("failed to read link startup result");
    if result? == 0 {
        let status = child
            .wait()
            .await
            .context("failed to wait for link startup")?;
        anyhow::bail!(
            "mcpmd link exited before readiness with {status}; see {:?}",
            endpoint.log_path()
        );
    }
    match serde_json::from_str::<Ready>(&message).context("invalid link readiness response")? {
        Ready::Ready | Ready::AlreadyRunning => {
            // Reap the child eventually without binding the daemon lifetime to this client.
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(())
        }
        Ready::Error(error) => {
            let _ = child.wait().await;
            Err(error.into_error()).context("failed to initialize SSH link")
        }
    }
}

fn daemon_executable() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to locate the running executable")?;
    let sibling = current.with_file_name(if cfg!(windows) { "mcpmd.exe" } else { "mcpmd" });
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Ok(PathBuf::from("mcpmd"))
    }
}
