use super::super::server::{agree, coordinate};
use crate::local::{
    mc::server::identify::{ServerLaunch, archive},
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
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
    if launch.main_class == "net.fabricmc.installer.ServerLauncher" {
        return automatic(launch).map(Some);
    }
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
    if let Some(jar) = &launch.jar {
        paths.push(jar.clone());
        if let Some(bytes) = archive::read(jar, "fabric-server-launch.properties")? {
            let props = archive::properties(&bytes)?;
            ensure!(
                matches!(
                    props.get("launch.mainClass").map(String::as_str),
                    Some(
                        "net.fabricmc.loader.impl.launch.knot.KnotServer"
                            | "net.fabricmc.loader.launch.knot.KnotServer"
                    )
                ),
                "unrecognized Fabric launch.mainClass"
            );
        }
    }
    let mut entry_verified = false;
    for path in &paths {
        if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader") {
            verify_loader(path, &version)?;
            agree(&mut loader, &version, "Fabric loader version")?;
            entry_verified |= archive::has(
                path,
                &format!("{}.class", launch.main_class.replace('.', "/")),
            )?;
        } else if let Some(version) = coordinate(path, "net.fabricmc", "intermediary") {
            ensure!(
                archive::has(path, "mappings/mappings.tiny")?,
                "referenced intermediary JAR has no mappings"
            );
            agree(&mut mc, &version, "Minecraft version")?;
        } else if Some(path) == launch.jar.as_ref() {
            // Old installers shaded the loader and intermediary into the launch JAR.
            if let Some(bytes) = archive::read(path, "fabric.mod.json")? {
                let json: serde_json::Value = serde_json::from_slice(&bytes)?;
                if json.get("id").and_then(|v| v.as_str()) == Some("fabricloader") {
                    let version = json
                        .get("version")
                        .and_then(|v| v.as_str())
                        .context("shaded Fabric version is missing")?;
                    agree(&mut loader, version, "Fabric loader version")?;
                    entry_verified |= archive::has(
                        path,
                        &format!("{}.class", launch.main_class.replace('.', "/")),
                    )?;
                }
            }
        }
    }
    ensure!(
        entry_verified,
        "Fabric entry class is absent from the referenced loader"
    );
    // The game JAR is selected by a JVM property or loader properties, never by scanning.
    let game = if let Some(value) = launch.properties.get("fabric.gameJarPath") {
        launch.resolve(value)
    } else {
        let props_path = launch.directory.join("fabric-server-launcher.properties");
        let props = match std::fs::read(&props_path) {
            Ok(bytes) => archive::properties(&bytes)?,
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
    let game_version =
        archive::minecraft_version(&game)?.context("Fabric's game JAR lacks version metadata")?;
    agree(&mut mc, &game_version, "Minecraft version")?;
    Ok(Some(ServerLoaderIdentity {
        minecraft_version: mc.unwrap(),
        version: loader.context("Fabric loader version is missing")?,
    }))
}

fn verify_loader(path: &std::path::Path, version: &str) -> Result<()> {
    let bytes = archive::read(path, "fabric.mod.json")?
        .context("referenced Fabric loader has no metadata")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(
        value.get("id").and_then(|v| v.as_str()) == Some("fabricloader"),
        "referenced JAR is not Fabric Loader"
    );
    ensure!(
        value.get("version").and_then(|v| v.as_str()) == Some(version),
        "Fabric loader metadata disagrees with Maven coordinate"
    );
    Ok(())
}

fn automatic(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    let jar = launch
        .jar
        .as_ref()
        .context("Fabric automatic launcher must be a JAR entry")?;
    ensure!(
        archive::has(jar, "net/fabricmc/installer/ServerLauncher.class")?,
        "Fabric automatic launcher class is missing"
    );
    ensure!(
        !launch.properties.contains_key("fabric.customLoaderPath"),
        "custom Fabric loader overrides cannot be identified automatically"
    );
    let props = archive::properties(
        &archive::read(jar, "install.properties")?
            .context("Fabric install.properties is missing")?,
    )?;
    let version = props
        .get("fabric-loader-version")
        .filter(|s| !s.trim().is_empty())
        .context("Fabric loader version is missing")?;
    let mc = props
        .get("game-version")
        .filter(|s| !s.trim().is_empty())
        .context("Fabric Minecraft version is missing")?;
    if let Some(custom_game) = launch.properties.get("fabric.installer.server.gameJar") {
        let found = archive::minecraft_version(&launch.resolve(custom_game))?
            .context("custom Fabric game JAR lacks version metadata")?;
        ensure!(
            found == *mc,
            "custom Fabric game JAR version conflicts with install.properties"
        );
    }
    Ok(ServerLoaderIdentity {
        minecraft_version: mc.clone(),
        version: version.clone(),
    })
}
