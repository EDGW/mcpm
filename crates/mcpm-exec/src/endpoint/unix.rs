//! Unix sockets with private directories, peer credentials, and a lifetime instance lock.

use super::AlreadyRunning;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};
use tokio::net::{
    UnixListener, UnixStream,
    unix::{OwnedReadHalf, OwnedWriteHalf},
};

#[derive(Clone, Debug)]
pub struct Endpoint {
    path: PathBuf,
    key: String,
}

impl Endpoint {
    pub fn for_key(key: &str) -> Result<Self> {
        super::validate_key(key)?;
        for directory in Self::directories() {
            let path = directory.join(format!("{key}.sock"));
            let limit = if cfg!(target_os = "macos") { 104 } else { 108 };
            if path.as_os_str().as_encoded_bytes().len() >= limit {
                continue;
            }
            private_directory(directory.parent().context("missing endpoint parent")?)?;
            private_directory(&directory)?;
            return Ok(Self {
                path,
                key: key.to_owned(),
            });
        }
        anyhow::bail!("no link endpoint directory fits the Unix socket path limit")
    }

    pub fn directories() -> Vec<PathBuf> {
        let uid = unsafe { libc::geteuid() };
        let mut candidates = Vec::new();
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty())
        {
            candidates.push(PathBuf::from(runtime).join("mcpm/link"));
        }
        if let Some(temporary) = std::env::var_os("TMPDIR").filter(|value| !value.is_empty()) {
            candidates.push(PathBuf::from(temporary).join(format!("mcpm-{uid}/link")));
        }
        candidates.push(PathBuf::from(format!("/tmp/mcpm-{uid}/link")));
        candidates.dedup();
        candidates
    }

    pub fn state_path(&self) -> PathBuf {
        self.path.with_extension("json")
    }
    pub fn log_path(&self) -> PathBuf {
        self.path.with_extension("log")
    }
    pub fn key(&self) -> &str {
        &self.key
    }

    pub async fn connect(&self) -> Result<(OwnedReadHalf, OwnedWriteHalf)> {
        let stream = UnixStream::connect(&self.path)
            .await
            .with_context(|| format!("failed to connect to link endpoint {:?}", self.path))?;
        check_peer(&stream)?;
        Ok(stream.into_split())
    }

    pub fn listen(&self) -> Result<Listener> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))
            .context("failed to open link instance lock")?;
        if unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&lock),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(AlreadyRunning.into());
            }
            return Err(error).context("failed to acquire link instance lock");
        }
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to remove stale link socket"),
        }
        let socket = UnixListener::bind(&self.path).context("failed to listen on link socket")?;
        Ok(Listener {
            socket,
            path: self.path.clone(),
            _lock: lock,
        })
    }
}

pub struct Listener {
    socket: UnixListener,
    path: PathBuf,
    _lock: File,
}

impl Listener {
    pub async fn accept(&mut self) -> Result<(OwnedReadHalf, OwnedWriteHalf)> {
        let (stream, _) = self
            .socket
            .accept()
            .await
            .context("failed to accept link client")?;
        check_peer(&stream)?;
        Ok(stream.into_split())
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn check_peer(stream: &UnixStream) -> Result<()> {
    let credentials = stream
        .peer_cred()
        .context("failed to query link peer credentials")?;
    ensure!(
        credentials.uid() == unsafe { libc::geteuid() },
        "link peer belongs to another user"
    );
    Ok(())
}

fn private_directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(path)
    {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to create private link directory {path:?}"));
        }
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect link directory {path:?}"))?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "link directory is not a real directory: {path:?}"
    );
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
        "link directory must be owned by the current user with mode 0700: {path:?}"
    );
    Ok(())
}
