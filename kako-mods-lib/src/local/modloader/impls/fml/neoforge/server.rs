use super::super::server::{self, Family};
use crate::local::{mc::server::identify::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::Result;

pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    server::identify(launch, Family::NeoForge)
}
