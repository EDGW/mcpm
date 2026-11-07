//! The single registration list of all mcpm business operations.

use anyhow::Result;
use mcpm_exec::operation::Registry;

pub mod cache;
pub mod mods;
pub mod server;
pub mod workspace;

pub(crate) mod stream;

pub fn registry() -> Result<Registry> {
    let mut registry = Registry::new();
    registry.register::<workspace::Inspect>()?;
    registry.register::<workspace::RegisterRemote>()?;
    registry.register::<server::info::Info>()?;
    registry.register::<server::shim::Prepare>()?;
    registry.register::<server::shim::Launch>()?;
    registry.register::<server::shim::Inspect>()?;
    registry.register::<server::shim::SubscribeStatus>()?;
    registry.register::<server::shim::Command>()?;
    registry.register::<server::shim::SubscribeLogs>()?;
    registry.register::<server::shim::SubscribeCommands>()?;
    registry.register::<server::shim::Kill>()?;
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
    registry.register::<cache::Init>()?;
    registry.register::<cache::List>()?;
    registry.register::<cache::Info>()?;
    registry.register::<cache::Import>()?;
    registry.register::<cache::Export>()?;
    registry.register::<cache::Remove>()?;
    registry.register::<cache::Verify>()?;
    registry.register::<cache::RebuildIndex>()?;
    registry.register::<mods::digest::Digest>()?;
    registry.register::<mods::list::List>()?;
    registry.register::<mods::info::Info>()?;
    registry.register::<mods::deps::Deps>()?;
    registry.register::<mods::source::search::Search>()?;
    registry.register::<mods::source::files::Files>()?;
    registry.register::<mods::source::identify::Identify>()?;
    registry.register::<mods::cache::Cache>()?;
    registry.register::<cache::refresh::Meta>()?;
    registry.register::<cache::refresh::Ident>()?;
    registry.register::<mods::dep_graph::Build>()?;
    registry.register::<mods::dep_graph::Inspect>()?;
    registry.register::<mods::backup::Create>()?;
    registry.register::<mods::backup::Apply>()?;
    registry.register::<mods::backup::Verify>()?;
    registry.register::<mods::backup::Checksum>()?;
    registry.register::<mods::backup::VerifyWith>()?;
    registry.register::<mods::backup::Delete>()?;
    Ok(registry)
}
