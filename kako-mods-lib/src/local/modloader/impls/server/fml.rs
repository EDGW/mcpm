use super::{agree, coordinate};
use crate::local::{
    mc::server::identify::{ServerLaunch, archive},
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::local::modloader::impls) enum Family {
    Forge,
    NeoForge,
}

pub(in crate::local::modloader::impls) fn identify(
    launch: &ServerLaunch,
    family: Family,
) -> Result<Option<ServerLoaderIdentity>> {
    if launch.main_class == "net.minecraftforge.bootstrap.shim.Main" {
        return if family == Family::Forge {
            shim(launch).map(Some)
        } else {
            Ok(None)
        };
    }
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
    let mut paths = launch.classpath.clone();
    for property in [
        "legacyClassPath",
        "fml.gameLayerLibraries",
        "fml.pluginLayerLibraries",
    ] {
        if let Some(value) = launch.properties.get(property) {
            for part in value
                .split(launch.path_separator)
                .flat_map(|s| s.split(','))
            {
                if part.is_empty() {
                    continue;
                }
                paths.push(launch.resolve(part));
                // FML layer lists may name only the artifact beside the referenced args file.
                if !part.contains(['/', '\\']) {
                    for args in &launch.argument_files {
                        paths.push(args.parent().unwrap().join(part));
                    }
                }
            }
        }
    }
    let mut verified = false;
    let mut game_verified = false;
    let mut entry_verified = false;
    let entry_class = format!("{}.class", launch.main_class.replace('.', "/"));
    for path in paths {
        if !entry_verified && path.is_file() {
            entry_verified = archive::has(&path, &entry_class)?;
        }
        if coordinate(&path, "net.minecraft", "server").is_some()
            || coordinate(&path, "net.neoforged", "minecraft-server-patched").is_some()
        {
            if let Some(found) = archive::minecraft_version(&path)? {
                ensure!(
                    found == mc,
                    "Minecraft JAR version disagrees with launch arguments"
                );
                game_verified = true;
            }
        }
        if family == Family::Forge {
            ensure!(
                coordinate(&path, "net.neoforged", "neoforge").is_none()
                    && coordinate(&path, "net.neoforged", "forge").is_none(),
                "NeoForge artifacts conflict with Forge launch arguments"
            );
            if let Some(coordinate) = coordinate(&path, "net.minecraftforge", "fmlloader") {
                ensure!(
                    coordinate == format!("{mc}-{version}"),
                    "Forge artifact version disagrees with launch arguments"
                );
                ensure!(
                    archive::has(&path, "net/minecraftforge/fml/loading/FMLLoader.class")?,
                    "referenced Forge loader class is missing"
                );
                verified = true;
            }
        } else {
            let modern = coordinate(&path, "net.neoforged", "neoforge");
            let early = coordinate(&path, "net.neoforged", "forge");
            if let Some(coordinate) = modern.as_ref().or(early.as_ref()) {
                let expected = if early.is_some() {
                    format!("{mc}-{version}")
                } else {
                    version.to_owned()
                };
                ensure!(
                    *coordinate == expected,
                    "NeoForge artifact version disagrees with launch arguments"
                );
                let marker = if early.is_some() {
                    "net/minecraftforge/common/ForgeMod.class"
                } else {
                    "net/neoforged/neoforge/common/NeoForgeMod.class"
                };
                ensure!(
                    archive::has(&path, marker)?,
                    "referenced NeoForge implementation class is missing"
                );
                verified = true;
            }
        }
    }
    ensure!(
        verified,
        "no referenced loader implementation verifies the declared server version"
    );
    ensure!(
        game_verified,
        "no referenced Minecraft JAR verifies the declared game version"
    );
    ensure!(
        entry_verified,
        "referenced classpath lacks the Java server entry class"
    );
    Ok(Some(ServerLoaderIdentity {
        minecraft_version: mc.into(),
        version: version.into(),
    }))
}

fn shim(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    let jar = launch
        .jar
        .as_ref()
        .context("Forge shim must be a JAR entry")?;
    ensure!(
        archive::has(jar, "net/minecraftforge/bootstrap/shim/Main.class")?,
        "Forge shim entry class is missing"
    );
    ensure!(
        !launch
            .game_arguments
            .iter()
            .any(|arg| arg.starts_with("--launchTarget") || arg.starts_with("--fml.")),
        "Forge shim launch overrides are unsupported"
    );
    let props = archive::properties(
        &archive::read(jar, "bootstrap-shim.properties")?
            .context("Forge shim properties are missing")?,
    )?;
    ensure!(
        props.get("Main-Class").map(String::as_str)
            == Some("net.minecraftforge.bootstrap.ForgeBootstrap"),
        "unrecognized Forge shim main class"
    );
    ensure!(
        props.get("Arguments").map(String::as_str) == Some("--launchTarget forge_server"),
        "unrecognized Forge shim launch target"
    );
    let list = archive::read(jar, "bootstrap-shim.list")?
        .context("Forge shim dependency list is missing")?;
    let mut mc = None;
    let mut loader = None;
    let mut implementation_verified = false;
    let mut game_version = None;
    for line in std::str::from_utf8(&list)?.lines() {
        let columns: Vec<_> = line.split('\t').collect();
        ensure!(
            columns.len() == 3 && columns[0].len() == 64,
            "invalid Forge shim library list"
        );
        let path = launch.directory.join("libraries").join(columns[2]);
        if coordinate(&path, "net.minecraft", "server").is_some() {
            if let Some(version) = archive::minecraft_version(&path)? {
                agree(&mut game_version, &version, "Minecraft game JAR version")?;
            }
        }
        if let Some(version) = coordinate(&path, "net.minecraftforge", "forge") {
            let (game, version) = version
                .split_once('-')
                .context("Forge coordinate has no Minecraft version")?;
            agree(&mut mc, game, "Minecraft version")?;
            agree(&mut loader, version, "Forge version")?;
            // The patched game/server artifact may not contain Forge classes; universal does.
            if columns[2].ends_with("-universal.jar") {
                ensure!(
                    archive::has(&path, "net/minecraftforge/common/ForgeMod.class")?,
                    "Forge universal JAR lacks its implementation"
                );
                implementation_verified = true;
            } else {
                ensure!(path.is_file(), "referenced Forge artifact is missing");
            }
        }
    }
    ensure!(
        implementation_verified,
        "Forge shim has no verified universal implementation"
    );
    let mc = mc.context("Forge shim has no Minecraft coordinate")?;
    ensure!(
        game_version.as_deref() == Some(mc.as_str()),
        "Forge shim Minecraft coordinate lacks matching game JAR metadata"
    );
    Ok(ServerLoaderIdentity {
        minecraft_version: mc,
        version: loader.context("Forge shim has no Forge coordinate")?,
    })
}
