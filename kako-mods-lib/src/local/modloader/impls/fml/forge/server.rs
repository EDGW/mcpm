use super::super::super::utils::coordinate::{agree, coordinate};
use super::super::server::{self, Family};
use crate::local::{
    mc::server::identify::{ServerLaunch, archive},
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    if launch.main_class == "net.minecraftforge.bootstrap.shim.Main" {
        return identify_shim(launch).map(Some);
    }
    server::identify(launch, Family::Forge)
}

fn identify_shim(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    let jar = launch
        .jar
        .as_ref()
        .context("Forge shim must be a JAR entry")?;
    tracing::debug!(
        ?jar,
        "expanding Forge shim metadata because launch arguments lack versions"
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
    for line in std::str::from_utf8(&list)?.lines() {
        let columns: Vec<_> = line.split('\t').collect();
        ensure!(columns.len() == 3, "invalid Forge shim library list");
        let path = launch.directory.join("libraries").join(columns[2]);
        if let Some(version) = coordinate(&path, "net.minecraftforge", "forge") {
            let (game, version) = version
                .split_once('-')
                .context("Forge coordinate has no Minecraft version")?;
            agree(&mut mc, game, "Minecraft version")?;
            agree(&mut loader, version, "Forge version")?;
            tracing::debug!(
                ?path,
                minecraft_version = game,
                loader_version = version,
                "found Forge shim dependency coordinate"
            );
        }
    }
    let mc = mc.context("Forge shim has no Minecraft coordinate")?;
    Ok(ServerLoaderIdentity {
        minecraft_version: mc,
        version: loader.context("Forge shim has no Forge coordinate")?,
    })
}
