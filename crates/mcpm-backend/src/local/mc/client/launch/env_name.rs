//! Client launch environment variable names.

use std::ffi::{OsStr, OsString};

/// A child environment variable name, uppercased on Windows and preserved elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvName(OsString);

impl EnvName {
    pub fn new(name: impl Into<OsString>) -> Self {
        Self(normalize(name.into()))
    }
}

impl AsRef<OsStr> for EnvName {
    fn as_ref(&self) -> &OsStr {
        &self.0
    }
}

impl From<OsString> for EnvName {
    fn from(name: OsString) -> Self {
        Self::new(name)
    }
}

impl From<&OsStr> for EnvName {
    fn from(name: &OsStr) -> Self {
        Self::new(name)
    }
}

impl From<&str> for EnvName {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for EnvName {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

#[cfg(not(windows))]
fn normalize(name: OsString) -> OsString {
    name
}

#[cfg(windows)]
fn normalize(name: OsString) -> OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let mut uppercase = Vec::new();
    for character in char::decode_utf16(name.encode_wide()) {
        match character {
            Ok(character) => {
                for character in character.to_uppercase() {
                    uppercase.extend_from_slice(character.encode_utf16(&mut [0; 2]));
                }
            }
            // Keep unpaired surrogates representable by OsString intact.
            Err(error) => uppercase.push(error.unpaired_surrogate()),
        }
    }
    OsString::from_wide(&uppercase)
}

#[cfg(test)]
mod tests {
    use super::EnvName;
    use crate::local::mc::client::launch::ProcessOptions;

    #[test]
    fn mixed_case_removal_respects_host_environment_names() {
        let mut options = ProcessOptions::default();
        options
            .environment
            .insert(EnvName::new("path"), Some("custom".into()));
        options.environment.insert(EnvName::new("PATH"), None);

        assert_eq!(options.environment[&EnvName::new("PATH")], None);
        if cfg!(windows) {
            assert_eq!(options.environment.len(), 1);
            assert_eq!(EnvName::new("Path").as_ref(), "PATH");
        } else {
            assert_eq!(options.environment.len(), 2);
            assert_eq!(
                options.environment[&EnvName::new("path")],
                Some("custom".into())
            );
            assert_eq!(EnvName::new("Path").as_ref(), "Path");
        }
    }

    #[cfg(windows)]
    #[test]
    fn uppercase_preserves_unpaired_surrogates() {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let name = EnvName::new(OsString::from_wide(&[0x0061, 0xd800, 0x00e9]));
        assert_eq!(
            name.as_ref().encode_wide().collect::<Vec<_>>(),
            [0x0041, 0xd800, 0x00c9]
        );
    }
}
