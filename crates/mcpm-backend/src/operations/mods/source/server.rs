//! Host-owned compatibility defaults, with explicit per-field intent from CLI expressions.

use anyhow::{Context as _, Result};
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
            crate::workspace::require_current_if_registered(&self.path)?;
            return Ok(());
        }
        let server = crate::workspace::load_local(self.path)?;
        let loader = server
            .resolved_loader()
            .with_context(|| match &server.metadata().loader {
                mcpm_lib::local::mc::server::ServerLoader::Vanilla => format!(
                    "server {:?} is Vanilla and has no mod loader",
                    server.path()
                ),
                mcpm_lib::local::mc::server::ServerLoader::Unrecognized => {
                    format!("server {:?} has an unrecognized loader", server.path())
                }
                mcpm_lib::local::mc::server::ServerLoader::Modded(loader) => format!(
                    "server {:?} declares unsupported mod loader {:?}",
                    server.path(),
                    loader.loader_type
                ),
            })?;
        if need_version {
            versions.push(server.metadata().minecraft_version.clone());
        }
        if need_loader {
            loaders.push(loader.loader.id().as_str().to_owned());
        }
        Ok(())
    }
}
