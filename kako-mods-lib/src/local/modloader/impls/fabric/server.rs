//! Loader-specific server launch recognition.

use super::super::utils::coordinate::{agree, coordinate};
use crate::local::{
    java::{jar, properties},
    mc::server::ServerLaunch,
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

/// 1. Reject unsupported overrides on Fabric entries.
/// 2. Select a supported Fabric server entry.
///    - Installer entry: call [`automatic`].
///    - Ordinary server entry: inspect declared Maven paths.
///    - Other entries: return `None`.
/// 3. Read loader and Minecraft versions from declared coordinates.
///    - Missing loader version: read shaded metadata from the launch JAR.
///    - Missing game version: read metadata from the selected game JAR.
/// 4. Log and return the identity; missing or conflicting evidence returns an error.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    // Overrides that change loader identity or launch mode need their own recognition rules.
    if launch.main_class.starts_with("net.fabricmc.") {
        for property in [
            "fabric.development",
            "fabric.gameJarPath.server",
            "fabric.gameJarPath.client",
            "fabric.customLoaderPath",
        ] {
            ensure!(
                !launch.properties.contains_key(property),
                "unsupported Fabric launch override {property}"
            );
        }
    }
    // Installer launchers keep their version declarations inside the launch JAR.
    if launch.main_class == "net.fabricmc.installer.ServerLauncher" {
        return automatic(launch)
            .map(Some)
            .with_context(|| format!("failed to identify Fabric installer JAR {:?}", launch.jar));
    }
    // Ordinary Fabric server entry classes are sufficient loader identity evidence.
    if !matches!(
        launch.main_class.as_str(),
        "net.fabricmc.loader.impl.launch.server.FabricServerLauncher"
            | "net.fabricmc.loader.launch.server.FabricServerLauncher"
            | "net.fabricmc.loader.impl.launch.knot.KnotServer"
            | "net.fabricmc.loader.launch.knot.KnotServer"
    ) {
        return Ok(None);
    }
    let mut loader = None;
    let mut mc = None;
    let mut paths = launch.classpath.clone();
    // Coordinates provide version evidence without opening dependency JARs.
    if let Some(jar) = &launch.jar {
        paths.push(jar.clone());
    }
    for path in &paths {
        if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader") {
            tracing::debug!(?path, %version, "found Fabric loader coordinate");
            agree(&mut loader, &version, "Fabric loader version")?;
        } else if let Some(version) = coordinate(path, "net.fabricmc", "intermediary") {
            tracing::debug!(?path, %version, "found Fabric game version coordinate");
            agree(&mut mc, &version, "Minecraft version")?;
        }
    }
    // Older shaded launchers need embedded metadata only when coordinates are insufficient.
    if loader.is_none()
        && let Some(jar) = &launch.jar
    {
        tracing::debug!(?jar, "reading shaded Fabric loader version");
        if let Some(bytes) = jar::read(jar, "fabric.mod.json")? {
            let json: serde_json::Value = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse fabric.mod.json in {jar:?}"))?;
            if json.get("id").and_then(|v| v.as_str()) == Some("fabricloader") {
                let version = json
                    .get("version")
                    .and_then(|v| v.as_str())
                    .with_context(|| {
                        format!("shaded Fabric version is missing in {jar:?}!fabric.mod.json")
                    })?;
                agree(&mut loader, version, "Fabric loader version")?;
            }
        }
    }
    let loader = loader.context("Fabric loader version is missing")?;
    // Read the selected game JAR only when no intermediary coordinate supplies its version.
    if mc.is_none() {
        // The game JAR is selected by a JVM property or loader properties, never by scanning.
        let game = if let Some(value) = launch.properties.get("fabric.gameJarPath") {
            launch.resolve(value)
        } else {
            let props_path = launch.directory.join("fabric-server-launcher.properties");
            let props = match std::fs::read(&props_path) {
                Ok(bytes) => properties::parse(&bytes)
                    .with_context(|| format!("failed to parse {props_path:?}"))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
                Err(error) => {
                    return Err(error).with_context(|| format!("failed to read {props_path:?}"));
                }
            };
            launch.resolve(
                props
                    .get("serverJar")
                    .map(String::as_str)
                    .unwrap_or("server.jar"),
            )
        };
        tracing::debug!(
            ?game,
            "reading Fabric game version because no intermediary coordinate is declared"
        );
        let game_version = crate::local::mc::jar::minecraft_version(&game)?
            .context("Fabric's game JAR lacks version metadata")?;
        agree(&mut mc, &game_version, "Minecraft version")?;
    }
    let minecraft_version = mc.context("Fabric Minecraft version is missing")?;
    tracing::debug!(%minecraft_version, loader_version = %loader, "identified Fabric server from launch evidence");
    Ok(Some(ServerLoaderIdentity {
        minecraft_version,
        version: loader,
    }))
}

/// 1. Require an explicit installer launch JAR.
/// 2. Read loader and game versions from its embedded install.properties.
/// 3. Log and return the identity; missing or empty versions return an error.
fn automatic(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    let jar = launch
        .jar
        .as_ref()
        .context("Fabric automatic launcher must be a JAR entry")?;
    // The installer entry establishes identity; embedded properties supply versions.
    tracing::debug!(?jar, "reading Fabric installer version declarations");
    let props = properties::parse(
        &jar::read(jar, "install.properties")?.context("Fabric install.properties is missing")?,
    )
    .with_context(|| format!("failed to parse install.properties in {jar:?}"))?;
    let version = props
        .get("fabric-loader-version")
        .filter(|s| !s.trim().is_empty())
        .context("Fabric loader version is missing")?;
    let mc = props
        .get("game-version")
        .filter(|s| !s.trim().is_empty())
        .context("Fabric Minecraft version is missing")?;
    // Installation integrity and custom game JAR contents are outside identification.
    tracing::debug!(minecraft_version = %mc, loader_version = %version, "identified Fabric installer server");
    Ok(ServerLoaderIdentity {
        minecraft_version: mc.clone(),
        version: version.clone(),
    })
}
