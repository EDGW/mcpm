//! Common FML launch-argument evidence; loader-specific launchers stay with the loader.

use crate::local::{mc::server::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Forge,
    NeoForge,
}

/// Extracts FML family and versions from declarations without checking launch validity.
/// Explicit client targets are excluded; absent or unknown targets do not block
/// otherwise sufficient evidence. Conflicting identity declarations remain errors.
pub(super) fn identify(
    launch: &ServerLaunch,
    family: Family,
) -> Result<Option<ServerLoaderIdentity>> {
    // Handle only known ordinary FML entry points.
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
    // Any NeoForge-specific evidence selects the NeoForge family.
    let neo = neo_version.is_some()
        || group == Some("net.neoforged")
        || launch.main_class.starts_with("net.neoforged.");
    if neo != (family == Family::NeoForge) {
        // Leave recognition to the detector for the matching family.
        return Ok(None);
    }
    if let Some(group) = group {
        let expected = if neo {
            "net.neoforged"
        } else {
            "net.minecraftforge"
        };
        ensure!(
            group == expected,
            "conflicting FML loader group {group:?}; expected {expected}"
        );
    }
    if let Some(target) = launch.option("--launchTarget")? {
        ensure_server_target(target)?;
    }
    // Read versions, including the Forge argument name used by early NeoForge.
    let mc = launch
        .option("--fml.mcVersion")?
        .context("Minecraft version is missing")?;
    let version = neo_version
        .or(launch.option("--fml.forgeVersion")?)
        .context("loader version is missing")?;
    if let (Some(neo), Some(forge)) = (neo_version, launch.option("--fml.forgeVersion")?) {
        // Require old and new loader version arguments to agree when both occur.
        ensure!(
            neo == forge,
            "conflicting Forge and NeoForge version arguments: forgeVersion={forge:?}, neoForgeVersion={neo:?}"
        );
    }
    // Record the sufficient identity evidence without inspecting dependency JARs.
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

/// Client targets are evidence against server identity, rather than invalid commands.
pub(in crate::local::modloader::impls::fml) fn ensure_server_target(target: &str) -> Result<()> {
    ensure!(
        !matches!(
            target.to_ascii_lowercase().as_str(),
            "forgeclient"
                | "neoforgeclient"
                | "forge_client"
                | "neoforge_client"
                | "fmlclient"
                | "fml_client"
                | "forgeclientuserdev"
                | "neoforgeclientuserdev"
        ),
        "entry declares a client target, not a production server: {target:?}"
    );
    Ok(())
}
