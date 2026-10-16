//! Loader-specific server launch recognition.

use super::super::super::utils::coordinate::{agree, coordinate};
use super::super::server::{self, Family};
use crate::local::{
    java::{jar, properties},
    mc::server::ServerLaunch,
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
        return identify_shim(launch)
            .map(Some)
            .with_context(|| format!("failed to identify Forge shim JAR {:?}", launch.jar));
    }
    // Check ordinary launch evidence using the Forge family rules.
    server::identify(launch, Family::Forge)
}

/// Reads shim version declarations and relevant command-line overrides.
/// Embedded launch arguments supply defaults; explicit identity options take precedence.
/// Dependency existence, entry class presence and command validity are not checked.
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
    let props = properties::parse(
        &jar::read(jar, "bootstrap-shim.properties")?
            .context("Forge shim properties are missing")?,
    )
    .with_context(|| format!("failed to parse bootstrap-shim.properties in {jar:?}"))?;
    let mut defaults = launch.clone();
    defaults.game_arguments = props
        .get("Arguments")
        .map(|value| shlex::split(value).context("cannot extract Forge shim Arguments"))
        .transpose()?
        .unwrap_or_default();
    let option = |name: &str| -> Result<Option<&str>> {
        match launch.option(name)? {
            Some(value) => Ok(Some(value)),
            None => defaults.option(name),
        }
    };
    if let Some(target) = option("--launchTarget")? {
        server::ensure_server_target(target)?;
    }
    if let Some(group) = option("--fml.forgeGroup")? {
        ensure!(
            group == "net.minecraftforge",
            "conflicting Forge shim loader group {group:?}"
        );
    }
    ensure!(
        option("--fml.neoForgeVersion")?.is_none(),
        "conflicting NeoForge version on Forge shim"
    );
    let declared_mc = option("--fml.mcVersion")?;
    let declared_loader = option("--fml.forgeVersion")?;
    if let (Some(mc), Some(loader)) = (declared_mc, declared_loader) {
        return Ok(ServerLoaderIdentity {
            minecraft_version: mc.into(),
            version: loader.into(),
        });
    }
    // Read version evidence from the embedded list without opening its dependencies.
    let list =
        jar::read(jar, "bootstrap-shim.list")?.context("Forge shim dependency list is missing")?;
    let mut mc = None;
    let mut loader = None;
    for (index, line) in std::str::from_utf8(&list)
        .with_context(|| format!("invalid UTF-8 in bootstrap-shim.list in {jar:?}"))?
        .lines()
        .enumerate()
    {
        // Check the list structure without validating hashes or dependency existence.
        let columns: Vec<_> = line.split('\t').collect();
        ensure!(
            columns.len() == 3,
            "invalid Forge shim library list in {jar:?}!bootstrap-shim.list at line {}",
            index + 1
        );
        let path = launch.directory.join("libraries").join(columns[2]);
        if let Some(version) = coordinate(&path, "net.minecraftforge", "forge") {
            // Extract Minecraft-loader versions from Forge coordinates; skip other artifacts.
            let (game, version) = version
                .split_once('-')
                .with_context(|| format!("Forge coordinate {path:?} has no Minecraft version in {jar:?}!bootstrap-shim.list at line {}", index + 1))?;
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
    Ok(ServerLoaderIdentity {
        minecraft_version: declared_mc
            .map(str::to_owned)
            .or(mc)
            .context("Forge shim has no Minecraft version evidence")?,
        version: declared_loader
            .map(str::to_owned)
            .or(loader)
            .context("Forge shim has no Forge version evidence")?,
    })
}
