//! Shared FML family and version evidence from statically referenced declarations.

use std::path::{Path, PathBuf};

use super::super::utils::coordinate::{argument_coordinate, coordinate};
use crate::local::{
    mc::server::{ServerLaunch, launch},
    modloader::ServerLoaderIdentity,
};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Forge,
    NeoForge,
}

pub(super) struct FmlLaunch<'a> {
    pub(super) launch: &'a ServerLaunch,
    pub(super) defaults: Vec<String>,
    pub(super) paths: Vec<PathBuf>,
}

impl FmlLaunch<'_> {
    fn option(&self, name: &str) -> Result<Option<&str>> {
        match self.launch.option(name)? {
            Some(value) => Ok(Some(value)),
            None => launch::option(&self.defaults, name)
                .with_context(|| format!("embedded FML defaults for {name}")),
        }
    }
}

/// Only ordinary FML entry points use this view; specialized launchers supply defaults.
pub(super) fn view(launch: &ServerLaunch) -> Option<FmlLaunch<'_>> {
    matches!(
        launch.main_class(),
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
            | "cpw.mods.modlauncher.Launcher"
            | "net.neoforged.fml.startup.Server"
    )
    .then(|| FmlLaunch {
        launch,
        defaults: Vec::new(),
        paths: Vec::new(),
    })
}

pub(super) fn identify(view: &FmlLaunch<'_>) -> Result<Option<(Family, ServerLoaderIdentity)>> {
    identify_with(view, None, None)
}

pub(super) fn identify_expected(
    view: &FmlLaunch<'_>,
    family: Family,
    path_list: Option<&str>,
) -> Result<Option<ServerLoaderIdentity>> {
    Ok(identify_with(view, Some(family), path_list)?.map(|(_, identity)| identity))
}

fn identify_with(
    view: &FmlLaunch<'_>,
    expected: Option<Family>,
    path_list: Option<&str>,
) -> Result<Option<(Family, ServerLoaderIdentity)>> {
    let group = view.option("--fml.forgeGroup")?;
    let neo_version = view.option("--fml.neoForgeVersion")?;
    let mut family = None;
    match group {
        Some("net.minecraftforge") => {
            family_evidence(&mut family, Family::Forge, "--fml.forgeGroup")?
        }
        Some("net.neoforged") => {
            family_evidence(&mut family, Family::NeoForge, "--fml.forgeGroup")?
        }
        _ => {}
    }
    if neo_version.is_some() {
        family_evidence(&mut family, Family::NeoForge, "--fml.neoForgeVersion")?;
    }

    let launch = view.launch;
    let mut paths = launch.classpath();
    paths.extend(launch.modulepath());
    if let Some(legacy) = launch.property("legacyClassPath") {
        paths.extend(
            legacy
                .split(launch.path_separator)
                .map(|path| launch.resolve(path)),
        );
    }
    let jar_paths = paths.iter().map(|path| (path, None)).chain(
        view.paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path, path_list.map(|name| (name, index + 1)))),
    );
    for (path, position) in jar_paths.clone() {
        for (group, artifact, value) in [
            ("net.minecraftforge", "fmlloader", Family::Forge),
            ("net.neoforged.fancymodloader", "loader", Family::NeoForge),
        ] {
            if coordinate(path, group, artifact).is_some() {
                family_evidence(&mut family, value, &path_source(path, position))?;
            }
        }
    }
    let Some((family, source)) = family else {
        return Ok(None);
    };
    if let Some(expected) = expected {
        ensure!(
            family == expected,
            "{expected:?} launcher declares the {family:?} family from {source}"
        );
    }

    let effective_mc = view.option("--fml.mcVersion")?;
    let mut minecraft = None;
    let mut loader = None;
    if let Some(value) = effective_mc {
        version_evidence(&mut minecraft, value, "Minecraft", "--fml.mcVersion")?;
    }
    for (name, value) in [
        ("--fml.forgeVersion", view.option("--fml.forgeVersion")?),
        ("--fml.neoForgeVersion", neo_version),
    ] {
        if let Some(value) = value {
            version_evidence(&mut loader, value, "loader", name)?;
        }
    }
    let group = match family {
        Family::Forge => "net.minecraftforge",
        Family::NeoForge => "net.neoforged",
    };
    // An args file can declare a distribution version, but is not an FML component JAR.
    let references = jar_paths
        .map(|(path, position)| (path, position, false))
        .chain(launch.argument_files.iter().map(|path| (path, None, true)));
    for (path, position, arguments) in references {
        let matcher = if arguments {
            argument_coordinate
        } else {
            coordinate
        };
        if let Some(value) = matcher(path, group, "forge") {
            let source = path_source(path, position);
            let known_mc = minecraft.as_ref().map(|(v, _)| v.as_str()).or(effective_mc);
            let (mc, version) = distribution_version(value, known_mc).with_context(|| {
                format!("invalid FML distribution version {value:?} in {source}")
            })?;
            version_evidence(&mut minecraft, mc, "Minecraft", &source)?;
            version_evidence(&mut loader, version, "loader", &source)?;
        }
        if family == Family::NeoForge
            && let Some(value) = matcher(path, group, "neoforge")
        {
            let source = path_source(path, position);
            version_evidence(&mut loader, value, "loader", &source)?;
        }
    }
    Ok(Some((
        family,
        ServerLoaderIdentity {
            minecraft_version: minecraft.context("Minecraft version is missing")?.0,
            version: loader.context("loader version is missing")?.0,
        },
    )))
}

fn path_source(path: &Path, position: Option<(&str, usize)>) -> String {
    if let Some((list, line)) = position {
        format!("{} ({list} line {line})", path.display())
    } else {
        path.display().to_string()
    }
}

fn family_evidence(
    current: &mut Option<(Family, String)>,
    value: Family,
    source: &str,
) -> Result<()> {
    if let Some((existing, previous)) = current {
        ensure!(
            *existing == value,
            "conflicting FML family: {existing:?} from {previous} versus {value:?} from {source}"
        );
    } else {
        *current = Some((value, source.to_owned()));
    }
    Ok(())
}

fn version_evidence(
    current: &mut Option<(String, String)>,
    value: &str,
    field: &str,
    source: &str,
) -> Result<()> {
    ensure!(
        !value.trim().is_empty(),
        "empty {field} version from {source}"
    );
    if let Some((existing, previous)) = current {
        ensure!(
            existing == value,
            "conflicting {field} version: {existing:?} from {previous} versus {value:?} from {source}"
        );
    } else {
        *current = Some((value.to_owned(), source.to_owned()));
    }
    Ok(())
}

/// Splits a Forge/NeoForge distribution version into `(minecraft_version, loader_version)`.
fn distribution_version<'a>(
    value: &'a str,
    effective_mc: Option<&str>,
) -> Result<(&'a str, &'a str)> {
    if let Some(mc) = effective_mc.filter(|mc| !mc.trim().is_empty())
        && let Some(rest) = value.strip_prefix(mc)
        && let Some(loader) = rest.strip_prefix('-')
        && !loader.trim().is_empty()
    {
        return Ok((&value[..mc.len()], loader));
    }
    let (mc, loader) = value
        .rsplit_once('-')
        .context("expected Minecraft-loader version")?;
    ensure!(
        !mc.trim().is_empty() && !loader.trim().is_empty(),
        "expected non-empty Minecraft and loader versions"
    );
    Ok((mc, loader))
}
