//! Format-independent byte codecs with shared file loading and saving.

use std::{fs, path::Path};

use anyhow::{Context, Result};

/// Converts typed values to and from bytes; implementations own format validation.
pub trait Codec {
    /// Encodes a value, rejecting values unsupported by the format.
    fn encode(&self) -> Result<Vec<u8>>;

    /// Decodes and validates one complete encoded value.
    fn decode(data: &[u8]) -> Result<Self>
    where
        Self: Sized;

    /// Reads and decodes a file, preserving its path in error context.
    fn open(file: &Path) -> Result<Self>
    where
        Self: Sized,
    {
        let data =
            fs::read(file).with_context(|| format!("failed to read encoded data from {file:?}"))?;
        Self::decode(&data).with_context(|| format!("failed to decode data from {file:?}"))
    }

    /// Encodes before writing, creating or overwriting the destination file.
    ///
    /// The parent directory must exist. Writes are not atomic.
    fn save(&self, file: &Path) -> Result<()> {
        let data = self
            .encode()
            .with_context(|| format!("failed to encode data for {file:?}"))?;
        fs::write(file, data).with_context(|| format!("failed to write encoded data to {file:?}"))
    }
}
