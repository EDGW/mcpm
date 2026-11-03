//! Typed file expectations and incremental verification shared by download targets.

use std::{collections::HashSet, io::Read};

use anyhow::{Result, ensure};
use sha1::{Digest, Sha1};
use sha2::{Sha256, Sha512};
use url::Url;

/// A file's original location and immutable expectations, preserved across mirrors.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    url: Url,
    size: Option<u64>,
    checksums: Vec<Checksum>,
}

impl DownloadRequest {
    pub fn new(url: Url, size: Option<u64>, checksums: Vec<Checksum>) -> Result<Self> {
        validate_url(&url)?;
        let mut algorithms = HashSet::new();
        for checksum in &checksums {
            ensure!(
                algorithms.insert(checksum.algorithm),
                "duplicate download checksum algorithm {:?}",
                checksum.algorithm
            );
        }
        Ok(Self {
            url,
            size,
            checksums,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn checksums(&self) -> &[Checksum] {
        &self.checksums
    }
}

/// An upstream digest, validated once and normalized to lowercase hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    algorithm: ChecksumAlgorithm,
    value: String,
}

impl Checksum {
    pub fn new(algorithm: ChecksumAlgorithm, value: &str) -> Result<Self> {
        let length = match algorithm {
            ChecksumAlgorithm::Sha1 => 40,
            ChecksumAlgorithm::Sha256 => 64,
            ChecksumAlgorithm::Sha512 => 128,
        };
        ensure!(
            value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid {algorithm:?} download checksum"
        );
        Ok(Self {
            algorithm,
            value: value.to_ascii_lowercase(),
        })
    }

    pub fn algorithm(&self) -> ChecksumAlgorithm {
        self.algorithm
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Verifies existing bytes with the same incremental digest path used by downloads.
    /// Reader I/O errors are preserved separately from [`ChecksumMismatch`].
    pub fn verify_reader(&self, reader: &mut impl Read) -> Result<()> {
        let mut verifier = Verifier::checksums(None, std::slice::from_ref(self));
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            verifier.update(&buffer[..count])?;
        }
        verifier.finish()?;
        Ok(())
    }
}

/// Digest algorithms supplied by supported upstream download metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChecksumAlgorithm {
    Sha1,
    Sha256,
    Sha512,
}

pub(super) struct Verifier {
    size: u64,
    expected_size: Option<u64>,
    digests: Vec<(Checksum, Hasher)>,
}

impl Verifier {
    pub(super) fn received(&self) -> u64 {
        self.size
    }

    pub(super) fn new(request: &DownloadRequest) -> Self {
        Self::checksums(request.size, &request.checksums)
    }

    fn checksums(expected_size: Option<u64>, checksums: &[Checksum]) -> Self {
        Self {
            size: 0,
            expected_size,
            digests: checksums
                .iter()
                .map(|checksum| {
                    let hasher = match checksum.algorithm {
                        ChecksumAlgorithm::Sha1 => Hasher::Sha1(Sha1::new()),
                        ChecksumAlgorithm::Sha256 => Hasher::Sha256(Sha256::new()),
                        ChecksumAlgorithm::Sha512 => Hasher::Sha512(Sha512::new()),
                    };
                    (checksum.clone(), hasher)
                })
                .collect(),
        }
    }

    pub(super) fn update(&mut self, bytes: &[u8]) -> Result<()> {
        self.size = self
            .size
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("download size overflow"))?;
        if let Some(expected) = self.expected_size
            && self.size > expected
        {
            return Err(SizeMismatch {
                expected,
                actual: self.size,
            }
            .into());
        }
        for (_, hasher) in &mut self.digests {
            match hasher {
                Hasher::Sha1(hasher) => hasher.update(bytes),
                Hasher::Sha256(hasher) => hasher.update(bytes),
                Hasher::Sha512(hasher) => hasher.update(bytes),
            }
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<u64> {
        if let Some(expected) = self.expected_size
            && self.size != expected
        {
            return Err(SizeMismatch {
                expected,
                actual: self.size,
            }
            .into());
        }
        for (checksum, hasher) in self.digests {
            let actual = match hasher {
                Hasher::Sha1(hasher) => format!("{:x}", hasher.finalize()),
                Hasher::Sha256(hasher) => format!("{:x}", hasher.finalize()),
                Hasher::Sha512(hasher) => format!("{:x}", hasher.finalize()),
            };
            if actual != checksum.value {
                return Err(ChecksumMismatch {
                    algorithm: checksum.algorithm,
                    expected: checksum.value,
                    actual,
                }
                .into());
            }
        }
        Ok(self.size)
    }
}

enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
    Sha512(Sha512),
}

pub(super) fn validate_url(url: &Url) -> Result<()> {
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "download URL must use HTTP(S): {url}"
    );
    Ok(())
}

/// Received bytes do not match an upstream digest; the file is never published.
#[derive(Debug, thiserror::Error)]
#[error("download {algorithm:?} checksum mismatch: expected {expected}, received {actual}")]
pub struct ChecksumMismatch {
    pub algorithm: ChecksumAlgorithm,
    pub expected: String,
    pub actual: String,
}

/// Received bytes do not match the size declared by upstream metadata.
#[derive(Debug, thiserror::Error)]
#[error("download size mismatch: expected {expected} bytes, received {actual}")]
pub struct SizeMismatch {
    pub expected: u64,
    pub actual: u64,
}
