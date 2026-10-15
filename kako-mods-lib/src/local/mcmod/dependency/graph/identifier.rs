use std::fmt::Debug;

use crate::local::modloader::{LoaderId, ModLoaderMeta};

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct DependencyIdentifier {
    loader: LoaderId,
    id: String,
}

impl Debug for DependencyIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}:{}", self.loader.as_str(), self.id))
    }
}

impl DependencyIdentifier {
    pub fn new(loader: &ModLoaderMeta, id: String) -> Self {
        Self::from_parts(loader.id().clone(), id)
    }

    /// Creates an identifier without requiring a runtime loader instance.
    pub fn from_parts(loader: LoaderId, id: String) -> Self {
        Self { loader, id }
    }
    pub fn to_string(&self) -> String {
        format!("{}:{}", self.loader.as_str(), self.id)
    }
    pub fn loader(&self) -> &LoaderId {
        &self.loader
    }
    pub fn id(&self) -> &str {
        &self.id
    }
}
