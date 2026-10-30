//! Deterministic runtime paths for a server instance.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct InstancePaths {
    root: PathBuf,
    runtime: PathBuf,
    socket: PathBuf,
}

impl InstancePaths {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_owned();
        let runtime = root.join(".mcpm").join("runtime");
        let digest = blake3::hash(root.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        let socket = if cfg!(windows) {
            PathBuf::from(format!(r"\\.\pipe\mcpm-instance-{}", &digest[..32]))
        } else {
            base.join("mcpm")
                .join("instance")
                .join(&digest[..32])
                .join("sock")
        };
        Self {
            root,
            runtime,
            socket,
        }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }
    pub fn lock(&self) -> PathBuf {
        self.runtime.join("lock")
    }
    pub fn state(&self) -> PathBuf {
        self.runtime.join("state.json")
    }
    pub fn socket(&self) -> &Path {
        &self.socket
    }
    pub fn socket_identity(&self) -> String {
        self.socket.to_string_lossy().into_owned()
    }
}
