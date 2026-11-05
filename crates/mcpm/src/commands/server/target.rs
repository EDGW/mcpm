//! Local-only guard retained for commands outside the migrated Mod flow.

use anyhow::{Result, ensure};

pub(super) fn require_supported_local(path: &std::path::Path, command: &str) -> Result<()> {
    ensure!(
        mcpm_backend::workspace::kind(path)? == mcpm_backend::workspace::Kind::Local,
        "{command} does not support Remote workspaces"
    );
    Ok(())
}
