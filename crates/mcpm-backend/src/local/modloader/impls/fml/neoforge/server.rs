//! Loader-specific server launch recognition.

use super::super::server::{self, Family};
use crate::local::{mc::server::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::Result;

/// 1. Call [`server::identify`] with the NeoForge family.
/// 2. Return the shared detector's result.
///    - Recognized: return `Some(identity)`.
///    - Unrelated entry: return `None`.
///    - Invalid required evidence: return an error.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    // Select NeoForge; shared logic filters other families and extracts versions.
    server::identify(launch, Family::NeoForge)
}
