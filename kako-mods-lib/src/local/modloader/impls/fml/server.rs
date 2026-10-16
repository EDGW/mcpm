//! Common FML launch-argument evidence; loader-specific launchers stay with the loader.

use crate::local::{mc::server::ServerLaunch, modloader::ServerLoaderIdentity};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Forge,
    NeoForge,
}

/// 1. Check the FML entry.
///    - Unsupported: return `None`.
///    - Supported: infer Forge/NeoForge.
/// 2. Compare the inferred and requested families.
///    - Different: return `None`.
///    - Matching: check the group and production server target.
///      - Direct NeoForge server entry: the target may be omitted.
///      - Other entries: require a supported server target.
/// 3. Read versions with [`ServerLaunch::option`].
///    - neoForgeVersion present: prefer it; require forgeVersion to agree if present.
///    - Otherwise: require forgeVersion.
/// 4. Log and return the identity without inspecting dependency JARs.
///    - Missing, invalid, duplicate, or conflicting required evidence: return an error.
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
    if !neo {
        // Shared entry classes require an explicit Forge group.
        ensure!(
            group == Some("net.minecraftforge"),
            "Forge group is absent or unrecognized: expected net.minecraftforge, got {group:?}"
        );
    }
    if neo && let Some(group) = group {
        // Allow an omitted NeoForge group, but reject contradictory declarations.
        ensure!(
            group == "net.neoforged",
            "NeoForge version conflicts with Forge group: expected net.neoforged, got {group:?}"
        );
    }
    if let Some(target) = launch.option("--launchTarget")? {
        // Accept production server target names used across FML generations.
        ensure!(
            matches!(
                target,
                "forgeserver" | "neoforgeserver" | "forge_server" | "neoforge_server"
            ),
            "entry is not a production server launch: unsupported --launchTarget {target:?}"
        );
    } else {
        // Only the direct server entry establishes its purpose without a target.
        ensure!(
            launch.main_class == "net.neoforged.fml.startup.Server",
            "server launch target is missing"
        );
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
