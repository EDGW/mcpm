//! Loader-specific server launch recognition.

use super::super::utils::coordinate::{agree, coordinate};
use crate::local::{
    java::{jar, properties},
    mc::server::ServerLaunch,
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result};
use std::path::Path;

/// 1. Read only overrides relevant to loader or game version evidence.
/// 2. Select a supported Fabric server entry.
///    - Installer entry: call [`automatic`].
///    - Ordinary server entry: inspect declared Maven paths.
///    - Other entries: return `None`.
/// 3. Read loader and Minecraft versions from declared coordinates.
///    - Missing loader version: read shaded metadata from the launch JAR.
///    - Missing game version: read metadata from the selected game JAR.
/// 4. Log and return the identity; missing or conflicting evidence returns an error.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
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
    let explicit_game = game_override(launch);
    let mut loader = None;
    let mut mc = None;
    let mut paths = launch.classpath.clone();
    // Coordinates provide version evidence without opening dependency JARs.
    if let Some(jar) = &launch.jar {
        paths.push(jar.clone());
    }
    for path in &paths {
        if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader")
            && !launch.properties.contains_key("fabric.customLoaderPath")
        {
            tracing::debug!(?path, %version, "found Fabric loader coordinate");
            agree(&mut loader, &version, "Fabric loader version")?;
        } else if let Some(version) = coordinate(path, "net.fabricmc", "intermediary")
            && explicit_game.is_none()
        {
            tracing::debug!(?path, %version, "found Fabric game version coordinate");
            agree(&mut mc, &version, "Minecraft version")?;
        }
    }
    // A selected custom loader replaces classpath loader evidence.
    let custom_loader = launch
        .properties
        .get("fabric.customLoaderPath")
        .map(|value| launch.resolve(value));
    if let Some(path) = &custom_loader {
        loader = loader_version(path)?;
    } else if loader.is_none()
        && let Some(jar) = &launch.jar
    {
        loader = loader_version(jar)?;
    }
    let loader = loader.context("Fabric loader version is missing")?;
    // Read the selected game JAR only when no intermediary coordinate supplies its version.
    if mc.is_none() || explicit_game.is_some() {
        // The game JAR is selected by a JVM property or loader properties, never by scanning.
        let game = if let Some(value) = explicit_game {
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
        tracing::debug!(?game, "reading selected Fabric game version metadata");
        let game_version = crate::local::mc::jar::minecraft_version(&game)?
            .context("Fabric's game JAR lacks version metadata")?;
        if explicit_game.is_some() {
            mc = Some(game_version);
        } else {
            agree(&mut mc, &game_version, "Minecraft version")?;
        }
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
    // Only read installer defaults for identity fields that have no explicit override.
    let custom_loader = launch.properties.get("fabric.customLoaderPath");
    let explicit_game = game_override(launch);
    let props = if custom_loader.is_some() && explicit_game.is_some() {
        Default::default()
    } else {
        properties::parse(
            &jar::read(jar, "install.properties")?
                .context("Fabric install.properties is missing")?,
        )
        .with_context(|| format!("failed to parse install.properties in {jar:?}"))?
    };
    let version = if let Some(path) = custom_loader {
        loader_version(&launch.resolve(path))?
            .context("selected Fabric loader version is missing")?
    } else {
        props
            .get("fabric-loader-version")
            .filter(|value| !value.trim().is_empty())
            .context("Fabric loader version is missing")?
            .clone()
    };
    let minecraft_version = if let Some(path) = explicit_game {
        crate::local::mc::jar::minecraft_version(&launch.resolve(path))?
            .context("selected Fabric game JAR lacks version metadata")?
    } else {
        props
            .get("game-version")
            .filter(|value| !value.trim().is_empty())
            .context("Fabric Minecraft version is missing")?
            .clone()
    };
    tracing::debug!(%minecraft_version, loader_version = %version, "identified Fabric installer server");
    Ok(ServerLoaderIdentity {
        minecraft_version,
        version,
    })
}

/// A server-specific game path takes precedence over the common game path.
fn game_override(launch: &ServerLaunch) -> Option<&str> {
    launch
        .properties
        .get("fabric.gameJarPath.server")
        .or_else(|| launch.properties.get("fabric.gameJarPath"))
        .map(String::as_str)
}

/// Reads version evidence from the selected loader without validating its launchability.
fn loader_version(path: &Path) -> Result<Option<String>> {
    if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader") {
        return Ok(Some(version));
    }
    tracing::debug!(?path, "reading shaded Fabric loader version");
    let Some(bytes) = jar::read(path, "fabric.mod.json")? else {
        return Ok(None);
    };
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse fabric.mod.json in {path:?}"))?;
    if json.get("id").and_then(|value| value.as_str()) != Some("fabricloader") {
        return Ok(None);
    }
    let version = json
        .get("version")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("shaded Fabric version is missing in {path:?}!fabric.mod.json"))?;
    Ok(Some(version.to_owned()))
}
