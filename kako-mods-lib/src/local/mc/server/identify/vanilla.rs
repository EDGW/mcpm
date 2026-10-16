//! Vanilla server recognition from parsed launch evidence and game JAR metadata.

use anyhow::{Context, Result, ensure};

use crate::local::{java::jar, mc::server::ServerLaunch};

pub(super) fn vanilla(launch: &ServerLaunch) -> Result<Option<String>> {
    let Some(jar) = &launch.jar else {
        return Ok(None);
    };
    if !matches!(
        launch.main_class.as_str(),
        "net.minecraft.bundler.Main"
            | "net.minecraft.server.Main"
            | "net.minecraft.server.MinecraftServer"
    ) {
        return Ok(None);
    }
    ensure!(
        launch.classpath.is_empty(),
        "vanilla launch with extra manifest classpath is unverified"
    );
    let class = format!("{}.class", launch.main_class.replace('.', "/"));
    ensure!(jar::has(jar, &class)?, "vanilla entry class is missing");
    let version = crate::local::mc::jar::minecraft_version(jar)?
        .context("vanilla JAR lacks verifiable Minecraft version metadata")?;
    Ok(Some(version))
}
