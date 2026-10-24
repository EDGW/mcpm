//! BLAKE3 content identities and shared imohash sampling for bytes and pinned readers.

use std::{
    fmt,
    io::{self, Cursor, Read, Seek, SeekFrom},
    str::FromStr,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Computes the BLAKE3-256 digest of all supplied bytes.
pub fn blake3(data: &[u8]) -> Result<Blake3Hash> {
    Ok(Blake3Hash::new(*blake3::hash(data).as_bytes()))
}

/// Hashes a complete reader from its current position with bounded memory.
pub fn blake3_reader(reader: &mut impl Read) -> io::Result<Blake3Hash> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(reader)?;
    Ok(Blake3Hash::new(*hasher.finalize().as_bytes()))
}

/// A full content identity, canonically represented by 64 lowercase hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Blake3Hash([u8; 32]);

impl Blake3Hash {
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        blake3::Hash::from_bytes(self.0).to_hex().fmt(f)
    }
}

impl FromStr for Blake3Hash {
    type Err = ParseBlake3HashError;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(ParseBlake3HashError);
        }
        blake3::Hash::from_hex(value)
            .map(|hash| Self::new(*hash.as_bytes()))
            .map_err(|_| ParseBlake3HashError)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected a BLAKE3 digest of 64 lowercase hexadecimal characters")]
pub struct ParseBlake3HashError;

impl Serialize for Blake3Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Blake3Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Computes standard imohash-1.0 using the same sampling protocol as file readers.
/// Sampling can miss changes and must never establish content equality on its own.
pub fn imohash(data: &[u8]) -> Result<ImoHash> {
    imohash_reader(&mut Cursor::new(data)).context("failed to compute imohash")
}

/// Samples a pinned reader: 16 KiB at start, midpoint and end for sizes >= 128 KiB.
/// Smaller contents are read completely. The reader position changes.
pub fn imohash_reader(reader: &mut (impl Read + Seek)) -> io::Result<ImoHash> {
    const SAMPLE_SIZE: u64 = 16 * 1024;
    const SAMPLE_THRESHOLD: u64 = 128 * 1024;
    let size = reader.seek(SeekFrom::End(0))?;
    reader.rewind()?;
    let mut samples = Vec::with_capacity(SAMPLE_THRESHOLD as usize);
    if size < SAMPLE_THRESHOLD {
        reader.take(size).read_to_end(&mut samples)?;
        if samples.len() as u64 != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "content changed during imohash",
            ));
        }
    } else {
        for offset in [0, size / 2, size - SAMPLE_SIZE] {
            reader.seek(SeekFrom::Start(offset))?;
            let start = samples.len();
            samples.resize(start + SAMPLE_SIZE as usize, 0);
            reader.read_exact(&mut samples[start..])?;
        }
    }
    let value = murmur3::murmur3_x64_128(&mut Cursor::new(samples), 0)?;
    let mut bytes = value.rotate_right(64).swap_bytes().to_le_bytes();
    let mut remaining = size;
    let mut offset = 0;
    while remaining >= 0x80 {
        bytes[offset] = remaining as u8 | 0x80;
        remaining >>= 7;
        offset += 1;
    }
    bytes[offset] = remaining as u8;
    Ok(ImoHash::new(bytes))
}

/// A sampling fingerprint, usable only to select candidates for equality checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImoHash([u8; 16]);

impl ImoHash {
    pub fn new(hash: [u8; 16]) -> Self {
        Self(hash)
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
