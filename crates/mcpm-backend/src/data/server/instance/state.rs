//! Atomic codec for the instance state file. Only the shim writes this file.
use super::paths::InstancePaths;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Running,
    Exited,
    Stopped,
    Lost,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExitRecord {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stopped: bool,
    pub at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceState {
    pub instance_id: String,
    pub shim_pid: u32,
    pub java_pid: u32,
    pub java_started_at: u64,
    pub socket: String,
    pub plan: String,
    pub status: InstanceStatus,
    #[serde(default)]
    pub exit: Option<ExitRecord>,
    #[serde(default)]
    pub shim_version: Option<String>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
pub fn read(paths: &InstancePaths) -> Result<Option<InstanceState>> {
    let path = paths.state();
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).context("invalid instance state.json")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}
pub fn write(paths: &InstancePaths, state: &InstanceState) -> Result<()> {
    fs::create_dir_all(paths.runtime()).context("failed to create instance runtime directory")?;
    let tmp = paths.state().with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state).context("failed to encode instance state")?;
    fs::write(&tmp, bytes).context("failed to write temporary instance state")?;
    fs::rename(&tmp, paths.state()).context("failed to publish instance state")
}

#[allow(dead_code)]
pub fn write_at(path: &Path, state: &InstanceState) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(path, bytes)?;
    Ok(())
}
