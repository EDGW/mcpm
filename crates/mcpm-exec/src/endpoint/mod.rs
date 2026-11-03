//! Per-user IPC endpoint discovery, security, and exclusive listeners.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// Validates the fixed hexadecimal endpoint identifier before using it in a path.
fn validate_key(key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        key.len() == 16 && key.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid link endpoint key"
    );
    Ok(())
}

/// Failure to exclusively own an endpoint; callers connect to the winning instance.
#[derive(Debug, thiserror::Error)]
#[error("link instance already exists")]
pub struct AlreadyRunning;
