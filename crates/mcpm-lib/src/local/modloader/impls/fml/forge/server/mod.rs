//! Forge server entry dispatch and manifest launch defaults.

use super::super::server::{self, Family, FmlLaunch};
use crate::local::{mc::server::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::{Context, Result};

mod legacy;
mod shim;

/// Identify Forge without treating an entry class alone as loader evidence.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    match launch.main_class() {
        "net.minecraftforge.bootstrap.shim.Main" => {
            let view = shim::view(launch)?;
            server::identify_expected(&view, Family::Forge, Some(shim::LIST))
                .with_context(|| format!("invalid Forge shim evidence in {:?}", launch.jar()))
        }
        "net.minecraftforge.server.ServerMain" => {
            let jar = launch
                .jar()
                .context("Forge ServerMain must be a JAR entry")?;
            let defaults = launch
                .jar_manifest
                .get("serverlaunchargs")
                .context("missing ServerLaunchArgs in META-INF/MANIFEST.MF")
                .and_then(|args| embedded_arguments(args, "ServerLaunchArgs"))
                .with_context(|| format!("invalid Forge ServerMain manifest in {jar:?}"))?;
            server::identify_expected(
                &FmlLaunch {
                    launch,
                    defaults,
                    paths: Vec::new(),
                },
                Family::Forge,
                None,
            )
            .with_context(|| format!("invalid Forge ServerMain evidence in {jar:?}"))
        }
        "cpw.mods.fml.relauncher.ServerLaunchWrapper"
        | "net.minecraftforge.fml.relauncher.ServerLaunchWrapper"
        | "net.minecraft.server.MinecraftServer" => legacy::identify(launch),
        _ => {
            let Some(view) = server::view(launch) else {
                return Ok(None);
            };
            Ok(server::identify(&view)?
                .filter(|(family, _)| *family == Family::Forge)
                .map(|(_, identity)| identity))
        }
    }
}

fn embedded_arguments(value: &str, attribute: &str) -> Result<Vec<String>> {
    shlex::split(value).with_context(|| format!("cannot split embedded {attribute}"))
}
