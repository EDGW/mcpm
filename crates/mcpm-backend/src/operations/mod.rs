//! The single registration list of all mcpm business operations.

use anyhow::Result;
use mcpm_exec::operation::Registry;

pub mod server;
pub mod workspace;

pub fn registry() -> Result<Registry> {
    let mut registry = Registry::new();
    registry.register::<workspace::Inspect>()?;
    registry.register::<workspace::RegisterRemote>()?;
    registry.register::<server::info::Info>()?;
    registry.register::<server::init::RecognizeServer>()?;
    registry.register::<server::init::Init>()?;
    registry.register::<server::init::prepare_remote::PrepareRemote>()?;
    registry.register::<server::stash::Create>()?;
    registry.register::<server::stash::Switch>()?;
    registry.register::<server::stash::Status>()?;
    registry.register::<server::stash::List>()?;
    registry.register::<server::stash::Info>()?;
    registry.register::<server::stash::Rename>()?;
    registry.register::<server::stash::Delete>()?;
    registry.register::<server::stash::Verify>()?;
    Ok(registry)
}
