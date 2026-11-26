//! Legacy Forge identification from the explicitly launched JAR's constants.

use anyhow::{Context, Result, bail, ensure};

use crate::local::{
    java::{
        class::{self, ConstantField},
        jar,
    },
    mc::server::{ServerLaunch, properties},
    modloader::ServerLoaderIdentity,
};

const FORGE_VERSION: &str = "net/minecraftforge/common/ForgeVersion.class";
const FML_VERSION: &str = "fmlversion.properties";
const MC_PROPERTY: &str = "fmlbuild.mcversion";
const LOADER_FIELDS: [&str; 4] = [
    "majorVersion",
    "minorVersion",
    "revisionVersion",
    "buildVersion",
];
const MC_FIELD: &str = "mcVersion";

pub(super) fn identify(launch: &ServerLaunch) -> Result<Option<ServerLoaderIdentity>> {
    let Some(path) = launch.jar() else {
        return Ok(None);
    };
    let mut archive = jar::open(&path)?;
    let Some(bytes) = archive
        .read(FORGE_VERSION, Some(jar::MAX_DOCUMENT))
        .with_context(|| format!("failed to read {FORGE_VERSION} in {path:?}"))?
    else {
        return Ok(None);
    };
    let [major, minor, revision, build] = LOADER_FIELDS;
    let fields = class::constant_fields(&bytes, &[major, minor, revision, build, MC_FIELD])
        .with_context(|| format!("invalid {FORGE_VERSION} in {path:?}"))?;
    let number = |name: &str| -> Result<i32> {
        match fields.get(name) {
            Some(ConstantField::Int(value)) if *value >= 0 => Ok(*value),
            Some(_) => bail!("invalid integer constant {name} in {FORGE_VERSION} in {path:?}"),
            None => bail!("missing integer constant {name} in {FORGE_VERSION} in {path:?}"),
        }
    };
    let version = format!(
        "{}.{}.{}.{}",
        number(major)?,
        number(minor)?,
        number(revision)?,
        number(build)?
    );
    let class_mc = match fields.get(MC_FIELD) {
        Some(ConstantField::String(value)) => {
            valid_mc(value)
                .with_context(|| format!("invalid mcVersion in {FORGE_VERSION} in {path:?}"))?;
            Some(value.as_ref())
        }
        Some(_) => bail!("invalid string constant mcVersion in {FORGE_VERSION} in {path:?}"),
        None => None,
    };
    let property_mc = archive
        .read(FML_VERSION, Some(jar::MAX_DOCUMENT))
        .with_context(|| format!("failed to read {FML_VERSION} in {path:?}"))?
        .map(|bytes| -> Result<Option<String>> {
            let mut properties = properties::parse_bytes(&bytes).with_context(|| {
                format!("failed to parse {MC_PROPERTY} in {FML_VERSION} in {path:?}")
            })?;
            let value = properties.remove(MC_PROPERTY);
            if let Some(value) = &value {
                valid_mc(value).with_context(|| {
                    format!("invalid {MC_PROPERTY} in {FML_VERSION} in {path:?}")
                })?;
            }
            Ok(value)
        })
        .transpose()?
        .flatten();
    if let (Some(class_mc), Some(property_mc)) = (class_mc, property_mc.as_deref()) {
        ensure!(
            class_mc == property_mc,
            "conflicting Minecraft versions in {path:?}: {FORGE_VERSION} mcVersion={class_mc:?}, {FML_VERSION} {MC_PROPERTY}={property_mc:?}"
        );
    }
    let minecraft_version = class_mc
        .map(str::to_owned)
        .or(property_mc)
        .with_context(|| format!("missing Minecraft version: mcVersion in {FORGE_VERSION} or {MC_PROPERTY} in {FML_VERSION} in {path:?}"))?;
    Ok(Some(ServerLoaderIdentity {
        minecraft_version,
        version,
    }))
}

fn valid_mc(value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "Minecraft version is empty");
    Ok(())
}
