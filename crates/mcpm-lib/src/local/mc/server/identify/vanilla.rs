//! Vanilla server recognition from parsed launch evidence and game JAR metadata.

use anyhow::{Context, Result};

use crate::local::mc::server::ServerLaunch;

pub(super) fn vanilla(launch: &ServerLaunch) -> Result<Option<String>> {
    let Some(jar) = &launch.jar() else {
        return Ok(None);
    };
    if !is_vanilla_main_class(launch.main_class()) {
        return Ok(None);
    }
    let version = crate::local::mc::jar::minecraft_version(jar)?
        .context("vanilla JAR lacks verifiable Minecraft version metadata")?;
    Ok(Some(version))
}

/// Positive entry-class evidence, shared with explicit registration upgrades.
pub(crate) fn is_vanilla_main_class(main_class: &str) -> bool {
    matches!(
        main_class,
        "net.minecraft.bundler.Main"
            | "net.minecraft.server.Main"
            | "net.minecraft.server.MinecraftServer"
    )
}
