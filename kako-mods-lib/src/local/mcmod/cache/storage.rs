//! Content fingerprints and the sole digest codec with atomic file publication.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::{CacheDigest, CacheError, CacheId, content::ContentFile, error::IoContext};

pub(super) fn exists(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) struct Fingerprint {
    pub id: CacheId,
    pub size: u64,
    pub sha256: String,
}

impl Fingerprint {
    pub fn matches(&self, digest: &CacheDigest) -> bool {
        self.id.to_string() == digest.blake3
            && self.size == digest.size
            && self.sha256 == digest.sha256
    }
}

pub(super) fn open_regular_file(source: &Path) -> Result<File, CacheError> {
    // Reject known non-regular objects before opening (e.g. blocking FIFOs).
    if !fs::metadata(source)
        .at("inspect data file", source)?
        .is_file()
    {
        return Err(CacheError::Io {
            operation: "read regular file",
            path: source.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "expected a regular file"),
        });
    }
    let input = File::open(source).at("open data file", source)?;
    if !input.metadata().at("inspect data file", source)?.is_file() {
        return Err(CacheError::Io {
            operation: "read regular file",
            path: source.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "expected a regular file"),
        });
    }
    Ok(input)
}

/// Hash exactly the bytes read, optionally streaming the same bytes into a copy.
pub(super) fn fingerprint(
    source: &Path,
    mut destination: Option<&mut File>,
) -> Result<Fingerprint, CacheError> {
    let mut input = ContentFile::open(source)?;
    input.file.rewind().at("rewind data file", source)?;
    let mut blake3 = blake3::Hasher::new();
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut buffer = [0; 65536];
    loop {
        let count = input.file.read(&mut buffer).at("read data file", source)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        blake3.update(&buffer[..count]);
        size += count as u64;
        if let Some(output) = destination.as_mut() {
            output
                .write_all(&buffer[..count])
                .at("write copied data", source)?;
        }
    }
    input.validate()?;
    Ok(Fingerprint {
        id: CacheId::from_hash(blake3.finalize()),
        size,
        sha256: format!("{:x}", hash.finalize()),
    })
}

pub(super) fn temporary(parent: &Path) -> Result<NamedTempFile, CacheError> {
    NamedTempFile::new_in(parent).at("create temporary file in", parent)
}

pub(super) fn publish(temp: NamedTempFile, path: &Path, replace: bool) -> Result<(), CacheError> {
    temp.as_file().sync_all().at("flush file", path)?;
    let result = if replace {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    result.map(|_| ()).map_err(|error| CacheError::Io {
        operation: "publish file",
        path: path.to_owned(),
        source: error.error,
    })
}

/// Moves a complete private staged file without replacing any existing name.
/// Uses native no-replace rename when supported, independent of import strategy.
pub(super) fn publish_staged(staged: &Path, destination: &Path) -> Result<(), CacheError> {
    tempfile::TempPath::try_from_path(staged)
        .at("prepare staged publication", staged)?
        .persist_noclobber(destination)
        .map_err(|error| CacheError::Io {
            operation: "publish cached data",
            path: destination.to_owned(),
            source: error.error,
        })
}

pub(super) fn read_digest(path: &Path) -> Result<CacheDigest, CacheError> {
    let bytes = fs::read(path).at("read cache digest", path)?;
    serde_json::from_slice(&bytes).map_err(|source| CacheError::Digest {
        path: path.to_owned(),
        source,
    })
}

pub(super) fn write_digest(
    path: &Path,
    digest: &CacheDigest,
    replace: bool,
) -> Result<bool, CacheError> {
    let mut bytes = serde_json::to_vec(digest).map_err(|source| CacheError::Digest {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    if replace {
        match fs::read(path) {
            Ok(existing) if existing == bytes => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            result => {
                result.at("read cache digest before publication", path)?;
            }
        }
    }
    let mut temp = temporary(path.parent().expect("cache digest has a parent"))?;
    temp.write_all(&bytes).at("write cache digest", path)?;
    publish(temp, path, replace)?;
    Ok(true)
}

pub(super) fn remove_if_exists(path: &Path) -> Result<bool, CacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        result => result.map(|_| true).at("remove file", path),
    }
}
