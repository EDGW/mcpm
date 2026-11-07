//! Source-owned transport codec for sensitive configuration and authentication values.

use std::fmt;

use mcpm_lib::secret::Secret;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A source credential whose wire representation retains its actual value.
///
/// Formatting delegates to the shared secret primitive; plaintext access remains
/// restricted to the source implementation.
#[derive(Clone)]
pub struct Credential(Secret);

impl Credential {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Secret::new(value))
    }

    pub(super) fn expose(&self) -> &str {
        self.0.expose()
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for Credential {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for Credential {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::mods::source::Configuration;

    #[test]
    fn credential_and_outer_configuration_formatting_cannot_reveal_secrets() {
        let credential = Credential::new("PRIVATE_CREDENTIAL_SENTINEL");
        let configuration = Configuration::from("curseforge@PRIVATE_CREDENTIAL_SENTINEL");
        for rendered in [
            format!("{credential} {credential:?} {credential:#?}"),
            format!("{configuration} {configuration:?} {configuration:#?}"),
            format!("{:?}", vec![configuration]),
        ] {
            assert!(!rendered.contains("PRIVATE_CREDENTIAL_SENTINEL"));
        }
    }
}
