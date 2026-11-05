//! Sensitive strings with explicit plaintext access and fixed redacted formatting.

use std::fmt;

/// A sensitive value that is never revealed by formatting.
///
/// Serialization belongs to the owning protocol, not this shared primitive.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the plaintext at an explicit credential-use boundary.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
