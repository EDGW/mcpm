//! ZIP opening and member reads shared by JAR consumers, with optional byte limits.

use std::{
    fs::File,
    io::{self, Read, Seek},
    path::Path,
};

use thiserror::Error;
use zip::{ZipArchive, result::ZipError};

pub(crate) trait ArchiveReader: Read + Seek {}
impl<T: Read + Seek> ArchiveReader for T {}

pub(crate) struct Archive {
    inner: ZipArchive<Box<dyn ArchiveReader>>,
}

impl Archive {
    pub(crate) fn open(path: &Path) -> Result<Self, ArchiveError> {
        Self::from_reader(File::open(path).map_err(ArchiveError::Io)?)
    }

    pub(crate) fn from_reader(reader: impl ArchiveReader + 'static) -> Result<Self, ArchiveError> {
        Ok(Self {
            inner: ZipArchive::new(Box::new(reader) as Box<dyn ArchiveReader>)
                .map_err(ArchiveError::Zip)?,
        })
    }

    pub(crate) fn has(&mut self, member: &str) -> Result<bool, ArchiveError> {
        match self.inner.by_name(member) {
            Ok(_) => Ok(true),
            Err(ZipError::FileNotFound) => Ok(false),
            Err(source) => Err(ArchiveError::Zip(source)),
        }
    }

    /// Missing members return None; corrupt archives and exceeded limits are errors.
    /// Limits apply to both declared and actual uncompressed sizes.
    pub(crate) fn read(
        &mut self,
        member: &str,
        limit: Option<u64>,
    ) -> Result<Option<Vec<u8>>, ArchiveError> {
        let mut file = match self.inner.by_name(member) {
            Ok(file) => file,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(source) => return Err(ArchiveError::Zip(source)),
        };
        let mut bytes = Vec::new();
        if let Some(limit) = limit {
            let exceeded = || ArchiveError::SizeLimit {
                member: member.to_owned(),
                limit,
            };
            if file.size() > limit {
                return Err(exceeded());
            }
            Read::by_ref(&mut file)
                .take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(ArchiveError::Io)?;
            if bytes.len() as u64 > limit {
                return Err(exceeded());
            }
        } else {
            file.read_to_end(&mut bytes).map_err(ArchiveError::Io)?;
        }
        Ok(Some(bytes))
    }
}

#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    #[error("archive I/O failed")]
    Io(#[source] io::Error),
    #[error("invalid or unreadable ZIP archive")]
    Zip(#[source] ZipError),
    #[error("archive member {member:?} exceeds the {limit} byte read limit")]
    SizeLimit { member: String, limit: u64 },
}
