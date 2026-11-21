//! Adapts embedded Forge bootstrap shim metadata to the shared FML launch view.

use anyhow::{Context, Result};

use super::{FmlLaunch, embedded_arguments};
use crate::local::{
    java::{jar, properties},
    mc::server::ServerLaunch,
};

pub(super) const LIST: &str = "bootstrap-shim.list";

pub(super) fn view(launch: &ServerLaunch) -> Result<FmlLaunch<'_>> {
    let path = launch.jar().context("Forge shim must be a JAR entry")?;
    let mut archive = jar::open(&path)?;
    let mut read = |member: &str| {
        archive
            .read(member, Some(jar::MAX_DOCUMENT))
            .with_context(|| format!("failed to read {member} in {path:?}"))?
            .with_context(|| format!("missing {member} in {path:?}"))
    };
    let properties = properties::parse(&read("bootstrap-shim.properties")?)
        .with_context(|| format!("failed to parse bootstrap-shim.properties in {path:?}"))?;
    let defaults = properties
        .get("Arguments")
        .map(|value| embedded_arguments(value, "bootstrap-shim.properties Arguments"))
        .transpose()
        .with_context(|| format!("invalid Forge shim defaults in {path:?}"))?
        .unwrap_or_default();
    let list = read(LIST)?;
    let libraries = launch.directory.join("libraries");
    let mut paths = Vec::new();
    let list = std::str::from_utf8(&list).map_err(|error| {
        let line = 1 + list[..error.valid_up_to()]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count();
        anyhow::Error::new(error)
            .context(format!("invalid UTF-8 in {path:?}!{LIST} at line {line}"))
    })?;
    for (index, line) in list.lines().enumerate() {
        let mut columns = line.splitn(3, '\t');
        let reference = columns
            .nth(2)
            .filter(|value| !value.trim().is_empty() && !value.contains('\t'))
            .with_context(|| {
                format!(
                    "invalid Forge shim library list in {path:?}!{LIST} at line {}",
                    index + 1
                )
            })?;
        paths.push(libraries.join(reference));
    }
    Ok(FmlLaunch {
        launch,
        defaults,
        paths,
    })
}
