//! Host-owned compatibility defaults, with explicit per-field intent from CLI expressions.

use crate::operations::server::info::ServerInfo;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A false field preserves explicit filters, including an explicitly cleared wildcard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub path: PathBuf,
    pub default_version: bool,
    pub default_loader: bool,
}
impl Context {
    pub(crate) fn apply(self, versions: &mut Vec<String>, loaders: &mut Vec<String>) -> Result<()> {
        let need_version = self.default_version && versions.is_empty();
        let need_loader = self.default_loader && loaders.is_empty();
        if !need_version && !need_loader {
            return Ok(());
        }
        let server = crate::workspace::load_local(self.path)?;
        let info = ServerInfo::from_server(&server);
        let loader = match info.loader {
            Some(loader) => loader,
            None => match info.declared_loader {
                None => bail!("server {:?} has no declared mod loader", info.path),
                Some(loader) => bail!(
                    "server {:?} declares unsupported mod loader {loader:?}",
                    info.path
                ),
            },
        };
        if need_version {
            versions.push(info.minecraft_version);
        }
        if need_loader {
            loaders.push(loader.id);
        }
        Ok(())
    }
}
