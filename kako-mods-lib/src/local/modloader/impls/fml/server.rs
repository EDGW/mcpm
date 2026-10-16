//! Common FML launch-argument evidence; loader-specific launchers stay with the loader.

use crate::local::{mc::server::identify::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Forge,
    NeoForge,
}

pub(super) fn identify(
    launch: &ServerLaunch,
    family: Family,
) -> Result<Option<ServerLoaderIdentity>> {
    if !matches!(
        launch.main_class.as_str(),
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
            | "cpw.mods.modlauncher.Launcher"
            | "net.neoforged.fml.startup.Server"
    ) {
        return Ok(None);
    }
    let group = launch.option("--fml.forgeGroup")?;
    let neo_version = launch.option("--fml.neoForgeVersion")?;
    let neo = neo_version.is_some()
        || group == Some("net.neoforged")
        || launch.main_class.starts_with("net.neoforged.");
    if neo != (family == Family::NeoForge) {
        return Ok(None);
    }
    if !neo {
        ensure!(
            group == Some("net.minecraftforge"),
            "Forge group is absent or unrecognized"
        );
    }
    if neo && let Some(group) = group {
        ensure!(
            group == "net.neoforged",
            "NeoForge version conflicts with Forge group"
        );
    }
    if let Some(target) = launch.option("--launchTarget")? {
        ensure!(
            matches!(
                target,
                "forgeserver" | "neoforgeserver" | "forge_server" | "neoforge_server"
            ),
            "entry is not a production server launch"
        );
    } else {
        ensure!(
            launch.main_class == "net.neoforged.fml.startup.Server",
            "server launch target is missing"
        );
    }
    let mc = launch
        .option("--fml.mcVersion")?
        .context("Minecraft version is missing")?;
    let version = neo_version
        .or(launch.option("--fml.forgeVersion")?)
        .context("loader version is missing")?;
    if let (Some(neo), Some(forge)) = (neo_version, launch.option("--fml.forgeVersion")?) {
        ensure!(
            neo == forge,
            "conflicting Forge and NeoForge version arguments"
        );
    }
    // The entry point and FML arguments identify the declared server. Dependencies
    // may be located dynamically by FML; identification does not verify installation.
    tracing::debug!(
        ?family,
        main_class = %launch.main_class,
        ?group,
        ?neo_version,
        minecraft_version = mc,
        loader_version = version,
        "identified FML server from launch arguments"
    );
    Ok(Some(ServerLoaderIdentity {
        minecraft_version: mc.into(),
        version: version.into(),
    }))
}
