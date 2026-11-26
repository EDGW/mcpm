//! Streaming lock-acquisition operation for local and remote concurrency debugging.

use std::{fmt, fs::File, path::PathBuf};

use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::mc::server::config::ServerLock;
use serde::{Deserialize, Serialize};

/// Acquires a real server OS file lock, emits [`ServerLockEvent::Acquired`], and holds it
/// until the consumer closes or drops the stream.
pub struct LockServer;

impl Operation for LockServer {
    const ID: &'static str = "debug.lock.server";
    type Input = ServerLockInput;
    type Output = StreamingOutput<ServerLockEvent>;

    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let (held_lock, path) = acquire(&input)?;
            if sender
                .send(ServerLockEvent::Acquired {
                    path,
                    name: input.name,
                    mode: input.mode,
                })
                .await
                .is_err()
            {
                drop(held_lock);
                return Ok(());
            }
            sender.closed().await;
            drop(held_lock);
            Ok(())
        }))
    }
}

/// Input parameters for [`LockServer`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerLockInput {
    pub server: PathBuf,
    pub name: ServerLockName,
    pub mode: ServerLockMode,
}

/// Which server lock file to acquire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerLockName {
    Server,
    Running,
}

impl ServerLockName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Running => "running",
        }
    }
}

impl fmt::Display for ServerLockName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether to acquire a shared or exclusive OS file lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerLockMode {
    Shared,
    Exclusive,
}

impl ServerLockMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Exclusive => "exclusive",
        }
    }
}

impl fmt::Display for ServerLockMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stream events emitted by [`LockServer`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerLockEvent {
    Acquired {
        path: PathBuf,
        name: ServerLockName,
        mode: ServerLockMode,
    },
}

enum HeldLock {
    Server { _lock: ServerLock },
    Running { file: File, path: PathBuf },
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        if let Self::Running { file, path } = self
            && let Err(error) = file.unlock()
        {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to release running lock"
            );
        }
    }
}

fn acquire(input: &ServerLockInput) -> Result<(HeldLock, PathBuf)> {
    match (input.name, input.mode) {
        (ServerLockName::Server, ServerLockMode::Exclusive) => {
            let lock = ServerLock::acquire_write(&input.server)?;
            let path = lock.path();
            Ok((HeldLock::Server { _lock: lock }, path))
        }
        (ServerLockName::Server, ServerLockMode::Shared) => {
            let lock = ServerLock::acquire_existing_read(&input.server)?;
            let path = lock.path();
            Ok((HeldLock::Server { _lock: lock }, path))
        }
        (ServerLockName::Running, ServerLockMode::Exclusive) => {
            let (file, path) = crate::server::shim::acquire_running_lock(&input.server, true)?;
            Ok((
                HeldLock::Running {
                    file,
                    path: path.clone(),
                },
                path,
            ))
        }
        (ServerLockName::Running, ServerLockMode::Shared) => {
            let (file, path) = crate::server::shim::acquire_running_lock(&input.server, false)?;
            Ok((
                HeldLock::Running {
                    file,
                    path: path.clone(),
                },
                path,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn debug_lock_server_enforces_real_locks_and_releases_on_stream_drop() {
        let root = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(root.path()).unwrap();

        // 1. Shared server lock without an existing .mcpm/server.lock must fail with zero writes.
        let mut missing_shared = mcpm_exec::operation::execute::<LockServer>(ServerLockInput {
            server: root.path().to_path_buf(),
            name: ServerLockName::Server,
            mode: ServerLockMode::Shared,
        })
        .await
        .unwrap();
        assert!(
            matches!(missing_shared.next().await, Some(Err(_))),
            "shared server lock must fail when .mcpm/server.lock does not exist"
        );
        assert!(
            !root.path().join(".mcpm").exists(),
            "failed shared server lock must not create .mcpm/"
        );

        // 2. Exclusive server lock blocks concurrent ServerLock::acquire_write while open and
        // releases immediately when the stream is dropped.
        let mut server_stream = mcpm_exec::operation::execute::<LockServer>(ServerLockInput {
            server: root.path().to_path_buf(),
            name: ServerLockName::Server,
            mode: ServerLockMode::Exclusive,
        })
        .await
        .unwrap();
        let event = server_stream
            .next()
            .await
            .expect("expected Acquired event")
            .expect("exclusive server lock should succeed");
        assert_eq!(
            event,
            ServerLockEvent::Acquired {
                path: canonical.join(".mcpm/server.lock"),
                name: ServerLockName::Server,
                mode: ServerLockMode::Exclusive,
            }
        );
        assert!(
            ServerLock::acquire_write(root.path()).is_err(),
            "exclusive server lock must block concurrent writes while stream is open"
        );
        drop(server_stream);
        tokio::task::yield_now().await;
        let reacquired = ServerLock::acquire_write(root.path())
            .expect("server lock must be released once stream is dropped");
        drop(reacquired);

        // 3. Exclusive running lock blocks shim::ensure_inactive_readonly while open and
        // unblocks once the stream is dropped.
        let mut running_stream = mcpm_exec::operation::execute::<LockServer>(ServerLockInput {
            server: root.path().to_path_buf(),
            name: ServerLockName::Running,
            mode: ServerLockMode::Exclusive,
        })
        .await
        .unwrap();
        let event = running_stream
            .next()
            .await
            .expect("expected Acquired event")
            .expect("exclusive running lock should succeed");
        assert_eq!(
            event,
            ServerLockEvent::Acquired {
                path: canonical.join(".mcpm/run/running.lock"),
                name: ServerLockName::Running,
                mode: ServerLockMode::Exclusive,
            }
        );
        assert!(
            crate::server::shim::ensure_inactive_readonly(root.path()).is_err(),
            "exclusive running lock must block ensure_inactive_readonly while stream is open"
        );
        drop(running_stream);
        tokio::task::yield_now().await;
        crate::server::shim::ensure_inactive_readonly(root.path())
            .expect("running lock must be released once stream is dropped");
    }
}
