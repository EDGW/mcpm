use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use zip::{ZipArchive, result::ZipError};

const MAX_DOCUMENT: u64 = 4 * 1024 * 1024;

pub(crate) fn read(path: &Path, member: &str) -> Result<Option<Vec<u8>>> {
    let mut archive =
        ZipArchive::new(File::open(path).with_context(|| format!("failed to open {path:?}"))?)
            .with_context(|| format!("invalid JAR {path:?}"))?;
    entry(&mut archive, member, MAX_DOCUMENT)
        .with_context(|| format!("failed to read {member} in {path:?}"))
}

fn entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    member: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>> {
    let mut file = match archive.by_name(member) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        file.size() <= limit,
        "archive member {member:?} exceeds identification size limit"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "archive member {member:?} exceeds identification size limit"
    );
    Ok(Some(bytes))
}

pub(crate) fn has(path: &Path, member: &str) -> Result<bool> {
    let mut archive =
        ZipArchive::new(File::open(path).with_context(|| format!("failed to open {path:?}"))?)
            .with_context(|| format!("invalid JAR {path:?}"))?;
    match archive.by_name(member) {
        Ok(_) => Ok(true),
        Err(ZipError::FileNotFound) => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {member} in {path:?}")),
    }
}

pub(crate) fn manifest(path: &Path) -> Result<HashMap<String, String>> {
    let bytes = read(path, "META-INF/MANIFEST.MF")?
        .with_context(|| format!("JAR {path:?} has no manifest"))?;
    (|| attributes(std::str::from_utf8(&bytes)?))()
        .with_context(|| format!("failed to parse META-INF/MANIFEST.MF in {path:?}"))
}

/// Main section only; continuation lines belong to their preceding attribute.
fn attributes(text: &str) -> Result<HashMap<String, String>> {
    let mut unfolded = Vec::<String>::new();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix(' ') {
            unfolded
                .last_mut()
                .context("orphan manifest continuation")?
                .push_str(rest);
        } else {
            unfolded.push(line.to_owned());
        }
    }
    let mut values = HashMap::new();
    for line in unfolded {
        let (key, value) = line
            .split_once(": ")
            .context("invalid manifest attribute")?;
        ensure!(
            values
                .insert(key.to_ascii_lowercase(), value.to_owned())
                .is_none(),
            "duplicate manifest attribute {key}"
        );
    }
    Ok(values)
}

/// Parses a conservative subset of java.util.Properties; exotic escapes are rejected.
pub(crate) fn properties(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    for (index, line) in std::str::from_utf8(bytes)?.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', '!']) {
            continue;
        }
        ensure!(
            !line.contains('\\'),
            "escaped or continued Java properties are unsupported at line {}",
            index + 1
        );
        let index = line
            .find(['=', ':'])
            .or_else(|| line.find(char::is_whitespace))
            .with_context(|| format!("invalid Java property at line {}", index + 1))?;
        let key = line[..index].trim();
        let value = line[index..]
            .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=' || ch == ':')
            .trim_end();
        ensure!(
            !key.is_empty() && values.insert(key.to_owned(), value.to_owned()).is_none(),
            "duplicate or empty property {key:?} at line {}",
            index + 1
        );
    }
    Ok(values)
}

pub(crate) fn manifest_paths(
    jar: &Path,
    manifest: &HashMap<String, String>,
) -> Result<Vec<PathBuf>> {
    manifest
        .get("class-path")
        .map(|cp| {
            cp.split_whitespace()
                .map(|value| {
                    ensure!(
                        !value.contains(':') && !value.contains('%'),
                        "URI or encoded manifest classpaths are unsupported in {jar:?}!META-INF/MANIFEST.MF: {value:?}"
                    );
                    Ok(jar.parent().unwrap().join(value))
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

pub(crate) fn minecraft_version(path: &Path) -> Result<Option<String>> {
    minecraft_version_inner(path)
        .with_context(|| format!("failed to read Minecraft version from {path:?}"))
}

fn minecraft_version_inner(path: &Path) -> Result<Option<String>> {
    if let Some(bytes) = read(path, "version.json")? {
        return Ok(Some(
            version_json(&bytes).context("failed to parse version.json")?,
        ));
    }
    let list_member = "META-INF/versions.list";
    let Some(list) = read(path, list_member)? else {
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
        let mut archive = ZipArchive::new(File::open(path)?)?;
        let bytes = entry(&mut archive, &member, 64 * 1024 * 1024)?
            .context("bundled Minecraft JAR is missing")?;
        let mut inner =
            ZipArchive::new(Cursor::new(bytes)).context("invalid bundled Minecraft JAR")?;
        let bytes = entry(&mut inner, "version.json", MAX_DOCUMENT)
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
