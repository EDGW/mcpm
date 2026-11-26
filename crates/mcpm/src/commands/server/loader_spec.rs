//! Shared `<loader>[:<version>]` CLI specification parser for `add-loader` and `new`.

use std::str::FromStr;

use anyhow::{Result, bail, ensure};
use mcpm_backend::operations::server::loader::LoaderRequest;
use mcpm_lib::download::loader::LoaderFamily;

/// Parsed `<forge|fabric|neoforge>[:<version>]` CLI argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoaderSpec {
    pub family: LoaderFamily,
    pub version: Option<String>,
}

impl LoaderSpec {
    /// Merges an optional inline `:version` with an explicit `--loader-version`,
    /// rejecting conflicting dual declarations.
    pub(super) fn into_request(self, explicit_version: Option<String>) -> Result<LoaderRequest> {
        let version = match (self.version, explicit_version) {
            (Some(_), Some(_)) => bail!(
                "loader version specified both inline ({}:<version>) and via --loader-version; specify it only once",
                self.family
            ),
            (Some(version), None) => Some(version),
            (None, Some(version)) => {
                let trimmed = version.trim();
                ensure!(!trimmed.is_empty(), "--loader-version cannot be empty");
                Some(trimmed.to_owned())
            }
            (None, None) => None,
        };
        Ok(LoaderRequest {
            family: self.family,
            version,
        })
    }
}

impl FromStr for LoaderSpec {
    type Err = String;

    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let (raw_family, raw_version) = match input.split_once(':') {
            Some((family, version)) => (family, Some(version)),
            None => (input, None),
        };
        let family = match raw_family.trim().to_ascii_lowercase().as_str() {
            "forge" => LoaderFamily::Forge,
            "fabric" => LoaderFamily::Fabric,
            "neoforge" => LoaderFamily::NeoForge,
            other => {
                return Err(format!(
                    "unsupported loader {other:?}; expected forge, fabric, or neoforge (optionally followed by :<version>)"
                ));
            }
        };
        let version = match raw_version {
            Some(version) => {
                let trimmed = version.trim();
                if trimmed.is_empty() {
                    return Err(format!(
                        "loader version after '{}:' cannot be empty",
                        family.as_str()
                    ));
                }
                Some(trimmed.to_owned())
            }
            None => None,
        };
        Ok(Self { family, version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_spec_parses_plain_and_inline_versions_and_rejects_conflicts() {
        let plain: LoaderSpec = "neoforge".parse().unwrap();
        assert_eq!(
            plain.clone().into_request(None).unwrap(),
            LoaderRequest {
                family: LoaderFamily::NeoForge,
                version: None,
            }
        );
        assert_eq!(
            plain.into_request(Some("21.1.72".into())).unwrap(),
            LoaderRequest {
                family: LoaderFamily::NeoForge,
                version: Some("21.1.72".into()),
            }
        );

        let inline: LoaderSpec = "forge:1.20.4-49.2.0".parse().unwrap();
        assert_eq!(
            inline.clone().into_request(None).unwrap(),
            LoaderRequest {
                family: LoaderFamily::Forge,
                version: Some("1.20.4-49.2.0".into()),
            }
        );
        assert!(inline.into_request(Some("49.2.0".into())).is_err());

        assert!("fabric:".parse::<LoaderSpec>().is_err());
        assert!("quilt:0.1.0".parse::<LoaderSpec>().is_err());
    }
}
