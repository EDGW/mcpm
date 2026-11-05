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
    Ok(registry)
}
