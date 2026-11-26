//! Loader-specific server launch recognition.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};

use super::super::utils::coordinate::{agree, coordinate};
use super::matching::SemanticVersion;
use crate::local::{
    java::jar,
    mc::server::{ServerLaunch, properties},
    modloader::ServerLoaderIdentity,
};

/// Fabric Loader 0.12.0+ supports `-Dfabric.gameJarPath` / `-Dfabric.gameJarPath.server`.
/// Earlier versions (e.g. 0.11.7) ignore/override the JVM property with `serverJar`
/// from `fabric-server-launcher.properties` (defaulting to `server.jar`).
pub fn supports_game_jar_property(loader_version: &str) -> Result<bool> {
    let version = SemanticVersion::parse(loader_version, false)
        .with_context(|| format!("invalid Fabric loader version {loader_version:?}"))?;
    let minimum = SemanticVersion::parse("0.12.0", false)
        .expect("static Fabric version 0.12.0 must parse");
    Ok(version.compare(&minimum).is_ge())
}

/// 1. Select a supported Fabric server entry.
///    - Installer entry: call [`automatic`].
///    - Ordinary server entry: inspect declared Maven paths.
///    - Other entries: return `None`.
/// 2. Resolve loader version from coordinates, `fabric.customLoaderPath`, or shaded metadata.
/// 3. Gate `-Dfabric.gameJarPath*` on Fabric Loader 0.12.0+ via [`supports_game_jar_property`].
/// 4. Resolve Minecraft version from the explicit game JAR, `intermediary` coordinates,
///    or `serverJar` in `fabric-server-launcher.properties` (defaulting to `server.jar`).
/// 5. Log and return the identity; missing or conflicting evidence returns an error.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    // Installer launchers keep their version declarations inside the launch JAR.
    if launch.main_class() == "net.fabricmc.installer.ServerLauncher" {
        return automatic(launch).map(Some).with_context(|| {
            format!("failed to identify Fabric installer JAR {:?}", launch.jar())
        });
    }
    // Ordinary Fabric server entry classes are sufficient loader identity evidence.
    if !matches!(
        launch.main_class(),
        "net.fabricmc.loader.impl.launch.server.FabricServerLauncher"
            | "net.fabricmc.loader.launch.server.FabricServerLauncher"
            | "net.fabricmc.loader.impl.launch.knot.KnotServer"
            | "net.fabricmc.loader.launch.knot.KnotServer"
    ) {
        return Ok(None);
    }
    let mut loader = None;
    let mut intermediaries = Vec::new();
    let mut paths = launch.classpath();
    // Coordinates provide version evidence without opening dependency JARs.
    if let Some(jar) = &launch.jar() {
        paths.push(jar.clone());
    }
    for path in &paths {
        if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader")
            && launch.property("fabric.customLoaderPath").is_none()
        {
            tracing::debug!(?path, %version, "found Fabric loader coordinate");
            agree(&mut loader, version, "Fabric loader version")?;
        } else if let Some(version) = coordinate(path, "net.fabricmc", "intermediary") {
            tracing::debug!(?path, %version, "found Fabric game version coordinate");
            intermediaries.push(version);
        }
    }
    // A selected custom loader replaces classpath loader evidence.
    let custom_loader = launch
        .property("fabric.customLoaderPath")
        .map(|value| launch.resolve(value));
    if let Some(path) = &custom_loader {
        loader = loader_version(path)?;
    } else if loader.is_none()
        && let Some(jar) = &launch.jar()
    {
        loader = loader_version(jar)?;
    }
    let loader = loader.context("Fabric loader version is missing")?;
    let explicit_game = match game_override(launch) {
        Some(value) => supports_game_jar_property(&loader)?.then_some(value),
        None => None,
    };
    let minecraft_version = if let Some(value) = explicit_game {
        let game = launch.resolve(value);
        tracing::debug!(?game, "reading selected Fabric game version metadata");
        crate::local::mc::jar::minecraft_version(&game)?
            .context("Fabric's game JAR lacks version metadata")?
    } else {
        let mut mc = None;
        for version in intermediaries {
            agree(&mut mc, version, "Minecraft version")?;
        }
        // Read the selected game JAR only when no intermediary coordinate supplies its version.
        if let Some(version) = mc {
            version
        } else {
            // The game JAR is selected by loader properties, never by scanning.
            let props_path = launch.directory.join("fabric-server-launcher.properties");
            let props = match std::fs::read(&props_path) {
                Ok(bytes) => properties::parse_bytes(&bytes)
                    .with_context(|| format!("failed to parse {props_path:?}"))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
                Err(error) => {
                    return Err(error).with_context(|| format!("failed to read {props_path:?}"));
                }
            };
            let game = launch.resolve(
                props
                    .get("serverJar")
                    .map(String::as_str)
                    .unwrap_or("server.jar"),
            );
            tracing::debug!(?game, "reading selected Fabric game version metadata");
            crate::local::mc::jar::minecraft_version(&game)?
                .context("Fabric's game JAR lacks version metadata")?
        }
    };
    tracing::debug!(%minecraft_version, loader_version = %loader, "identified Fabric server from launch evidence");
    Ok(Some(ServerLoaderIdentity {
        minecraft_version,
        version: loader,
    }))
}

/// 1. Require an explicit installer launch JAR.
/// 2. Resolve loader version first from `fabric.customLoaderPath` or embedded `install.properties`.
/// 3. Gate `-Dfabric.gameJarPath*` on Fabric Loader 0.12.0+ via [`supports_game_jar_property`].
/// 4. Read game version from the explicit game JAR or embedded `install.properties`.
/// 5. Log and return the identity; missing or empty versions return an error.
fn automatic(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    let jar = launch
        .jar()
        .context("Fabric automatic launcher must be a JAR entry")?;
    let mut props = None;
    let version = if let Some(path) = launch.property("fabric.customLoaderPath") {
        loader_version(&launch.resolve(path))?
            .context("selected Fabric loader version is missing")?
    } else {
        install_properties(&mut props, &jar)?
            .get("fabric-loader-version")
            .filter(|value| !value.trim().is_empty())
            .context("Fabric loader version is missing")?
            .clone()
    };
    let explicit_game = match game_override(launch) {
        Some(value) => supports_game_jar_property(&version)?.then_some(value),
        None => None,
    };
    let minecraft_version = if let Some(path) = explicit_game {
        crate::local::mc::jar::minecraft_version(&launch.resolve(path))?
            .context("selected Fabric game JAR lacks version metadata")?
    } else {
        install_properties(&mut props, &jar)?
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

fn install_properties<'a>(
    cache: &'a mut Option<HashMap<String, String>>,
    jar: &Path,
) -> Result<&'a HashMap<String, String>> {
    if cache.is_none() {
        let parsed = properties::parse_bytes(
            &jar::read(jar, "install.properties")?
                .context("Fabric install.properties is missing")?,
        )
        .with_context(|| format!("failed to parse install.properties in {jar:?}"))?;
        *cache = Some(parsed);
    }
    Ok(cache.as_ref().expect("install.properties loaded"))
}

/// A server-specific game path takes precedence over the common game path.
fn game_override(launch: &ServerLaunch) -> Option<&str> {
    launch
        .property("fabric.gameJarPath.server")
        .or_else(|| launch.property("fabric.gameJarPath"))
}

/// Reads version evidence from the selected loader without validating its launchability.
fn loader_version(path: &Path) -> Result<Option<String>> {
    if let Some(version) = coordinate(path, "net.fabricmc", "fabric-loader") {
        return Ok(Some(version.to_owned()));
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

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::local::mc::server::launch::{JavaCommand, JavaEntry};

    fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
        let mut archive = ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn game_jar_property_boundary_between_0_11_7_and_0_12_0() {
        assert!(!supports_game_jar_property("0.11.7").unwrap());
        assert!(!supports_game_jar_property("0.12.0-beta.1").unwrap());
        assert!(supports_game_jar_property("0.12.0").unwrap());
        assert!(supports_game_jar_property("0.12.5").unwrap());
        assert!(supports_game_jar_property("0.16.0").unwrap());
        assert!(supports_game_jar_property("not-a-version").is_err());

        let root = tempfile::tempdir().unwrap();
        write_jar(
            &root.path().join("real.jar"),
            &[("version.json", br#"{"id":"1.16.5"}"#)],
        );
        write_jar(
            &root.path().join("wrong.jar"),
            &[("version.json", br#"{"id":"1.20.4"}"#)],
        );
        fs::write(
            root.path().join("fabric-server-launcher.properties"),
            "serverJar=real.jar\n",
        )
        .unwrap();

        let launch = ServerLaunch {
            executable: "java".into(),
            script: root.path().join("run.sh"),
            line: 1,
            directory: root.path().to_path_buf(),
            command: JavaCommand {
                jvm: vec!["-Dfabric.gameJarPath=wrong.jar".into()],
                entry: JavaEntry::Jar {
                    path: "fabric-server-launch.jar".into(),
                },
                application: Vec::new(),
            },
            environment: HashMap::new(),
            argument_files: Vec::new(),
            jar_manifest: HashMap::from([(
                "main-class".into(),
                "net.fabricmc.loader.launch.server.FabricServerLauncher".into(),
            )]),
            path_separator: ':',
        };

        write_jar(
            &root.path().join("fabric-server-launch.jar"),
            &[(
                "fabric.mod.json",
                br#"{"id":"fabricloader","version":"0.11.7"}"#,
            )],
        );
        assert_eq!(
            identify(&launch).unwrap(),
            Some(ServerLoaderIdentity {
                minecraft_version: "1.16.5".into(),
                version: "0.11.7".into(),
            })
        );

        write_jar(
            &root.path().join("fabric-server-launch.jar"),
            &[(
                "fabric.mod.json",
                br#"{"id":"fabricloader","version":"0.12.0"}"#,
            )],
        );
        assert_eq!(
            identify(&launch).unwrap(),
            Some(ServerLoaderIdentity {
                minecraft_version: "1.20.4".into(),
                version: "0.12.0".into(),
            })
        );
    }
}
