//! Loader-specific server launch recognition.

use super::super::server::{self, Family};
use crate::local::{mc::server::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::Result;

/// Identifies only NeoForge results from the shared ordinary FML entry points.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    let Some(view) = server::view(launch) else {
        return Ok(None);
    };
    Ok(server::identify(&view)?
        .and_then(|(family, identity)| (family == Family::NeoForge).then_some(identity)))
}
