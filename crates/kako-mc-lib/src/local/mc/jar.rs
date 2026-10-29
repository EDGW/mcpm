//! Minecraft version evidence in ordinary and bundled game JARs.

use std::{io::Cursor, path::Path};

use anyhow::{Context, Result, ensure};

use crate::local::{archive::zip::Archive, java::jar};

pub(crate) fn minecraft_version(path: &Path) -> Result<Option<String>> {
    minecraft_version_inner(path)
        .with_context(|| format!("failed to read Minecraft version from {path:?}"))
}

fn minecraft_version_inner(path: &Path) -> Result<Option<String>> {
    if let Some(bytes) = jar::read(path, "version.json")? {
        return Ok(Some(
            version_json(&bytes).context("failed to parse version.json")?,
        ));
    }
    let list_member = "META-INF/versions.list";
    let Some(list) = jar::read(path, list_member)? else {
        return Ok(None);
    };
    let text =
        std::str::from_utf8(&list).with_context(|| format!("invalid UTF-8 in {list_member}"))?;
    let rows: Vec<_> = text.lines().filter(|s| !s.trim().is_empty()).collect();
    ensure!(
        rows.len() == 1,
        "ambiguous bundled Minecraft versions in {list_member}: expected one row, got {}",
        rows.len()
    );
    let row: Vec<_> = rows[0].split('\t').collect();
    ensure!(
        row.len() == 3 && !row[1].is_empty(),
        "invalid bundled versions list {list_member}"
    );
    let member = format!("META-INF/versions/{}", row[2]);
    let bundled_version = (|| -> Result<String> {
        let mut archive = jar::open(path)?;
        let bytes = archive
            .read(&member, Some(64 * 1024 * 1024))?
            .context("bundled Minecraft JAR is missing")?;
        let mut inner =
            Archive::from_reader(Cursor::new(bytes)).context("invalid bundled Minecraft JAR")?;
        let bytes = inner
            .read("version.json", Some(jar::MAX_DOCUMENT))
            .context("failed to read bundled version.json")?
            .context("bundled game version.json is missing")?;
        version_json(&bytes).context("failed to parse bundled version.json")
    })()
    .with_context(|| format!("failed to inspect bundled JAR {member:?}"))?;
    ensure!(
        bundled_version == row[1],
        "bundled Minecraft version disagrees with version.json: {} declares {:?}, {member}!version.json declares {bundled_version:?}",
        list_member,
        row[1]
    );
    Ok(Some(bundled_version))
}

fn version_json(bytes: &[u8]) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .context("version.json has no string id")?;
    ensure!(!id.trim().is_empty(), "Minecraft version is empty");
    Ok(id.to_owned())
}
