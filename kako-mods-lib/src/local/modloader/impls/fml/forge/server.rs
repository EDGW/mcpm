use super::super::super::utils::coordinate::{agree, coordinate};
use super::super::server::{self, Family};
use crate::local::{
    mc::server::identify::{ServerLaunch, archive},
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

/// 1. Select the recognition path.
/// * Shim entry: call [`identify_shim`] to read embedded version declarations.
/// * Other entries: call [`server::identify`] with the Forge family.
/// 2. Return the selected detector's result.
/// * Recognized: return `Some(identity)`.
/// * Unrelated entry: return `None`.
/// * Invalid required evidence: return an error.
pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    // Expand shim metadata because its versions reside in the launch JAR.
    if launch.main_class == "net.minecraftforge.bootstrap.shim.Main" {
        return identify_shim(launch).map(Some);
    }
    // Check ordinary launch evidence using the Forge family rules.
    server::identify(launch, Family::Forge)
}

/// 1. Require an explicit launch JAR and reject FML argument overrides.
/// 2. Read shim properties; require the Forge bootstrap class and server target.
/// 3. Read the embedded dependency list.
///    - Forge Maven coordinate: extract versions and require consistency with [`agree`].
///    - Other coordinates: skip.
/// 4. Return the identity without opening dependency JARs.
///    - Missing or invalid required declarations: return an error.
fn identify_shim(launch: &ServerLaunch) -> Result<ServerLoaderIdentity> {
    // Read shim metadata from the explicitly selected launch JAR.
    let jar = launch
        .jar
        .as_ref()
        .context("Forge shim must be a JAR entry")?;
    tracing::debug!(
        ?jar,
        "expanding Forge shim metadata because launch arguments lack versions"
    );
    // Reject command-line overrides that could change the embedded declaration.
    ensure!(
        !launch
            .game_arguments
            .iter()
            .any(|arg| arg.starts_with("--launchTarget") || arg.starts_with("--fml.")),
        "Forge shim launch overrides are unsupported"
    );
    // Expand the shim declaration and require a Forge production server.
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
    // Read version evidence from the embedded list without opening its dependencies.
    let list = archive::read(jar, "bootstrap-shim.list")?
        .context("Forge shim dependency list is missing")?;
    let mut mc = None;
    let mut loader = None;
    for line in std::str::from_utf8(&list)?.lines() {
        // Check the list structure without validating hashes or dependency existence.
        let columns: Vec<_> = line.split('\t').collect();
        ensure!(columns.len() == 3, "invalid Forge shim library list");
        let path = launch.directory.join("libraries").join(columns[2]);
        if let Some(version) = coordinate(&path, "net.minecraftforge", "forge") {
            // Extract Minecraft-loader versions from Forge coordinates; skip other artifacts.
            let (game, version) = version
                .split_once('-')
                .context("Forge coordinate has no Minecraft version")?;
            // Require version declarations from multiple Forge artifacts to agree.
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
    // Require complete version evidence before returning an identity.
    let mc = mc.context("Forge shim has no Minecraft coordinate")?;
    Ok(ServerLoaderIdentity {
        minecraft_version: mc,
        version: loader.context("Forge shim has no Forge coordinate")?,
    })
}
