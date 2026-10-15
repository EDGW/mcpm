use std::{borrow::Borrow, collections::HashMap, fmt};

use super::ModLoaderMeta;

/// A case-sensitive loader identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoaderId(String);

impl LoaderId {
    /// Wraps a loader identifier without changing its contents.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Looks up this identifier in the supplied loader map.
    /// Returns `None` if the map does not contain this loader.
    pub fn metadata<'a>(
        &self,
        loaders: &'a HashMap<LoaderId, ModLoaderMeta>,
    ) -> Option<&'a ModLoaderMeta> {
        loaders.get(self)
    }
}

impl From<String> for LoaderId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for LoaderId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl Borrow<str> for LoaderId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for LoaderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for LoaderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
