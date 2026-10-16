use std::fs::File;
use std::io::{Read, Seek};

use anyhow::{Context, Result};
use zip::{ZipArchive, result::ZipError};

use crate::local::mcmod::ModEntry;

mod nested;
pub(in crate::local::modloader::impls) use nested::{ArchiveNode, collect};

pub(in crate::local::modloader::impls) trait ArchiveReader:
    Read + Seek
{
}
impl<T: Read + Seek> ArchiveReader for T {}
pub(in crate::local::modloader::impls) type Archive = ZipArchive<Box<dyn ArchiveReader>>;

pub(in crate::local::modloader::impls) fn open(entry: &ModEntry) -> Result<Archive> {
    let file = File::open(entry.path())
        .with_context(|| format!("failed to read mod file {:?}", entry.path()))?;
    ZipArchive::new(Box::new(file) as Box<dyn ArchiveReader>)
        .with_context(|| format!("invalid mod archive {:?}", entry.path()))
}

pub(in crate::local::modloader::impls) fn read(
    archive: &mut Archive,
    path: &str,
) -> Result<Option<String>> {
    let mut file = match archive.by_name(path) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("failed to open {path}")),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .with_context(|| format!("failed to read metadata document {path}"))?;
    Ok(Some(contents.trim_start_matches('\u{feff}').to_owned()))
}
