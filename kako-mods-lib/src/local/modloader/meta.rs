use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use super::{LoaderId, ModLoader};

/// A loader's immutable identity and the instance that provides its behavior.
/// Equality and hashing use only the stable identifier, compared case-sensitively.
#[derive(Debug, Clone)]
pub struct ModLoaderMeta {
    id: LoaderId,
    name: String,
    instance: Arc<dyn ModLoader>,
}

impl PartialEq for ModLoaderMeta {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ModLoaderMeta {}

impl Hash for ModLoaderMeta {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl ModLoaderMeta {
    /// Creates metadata with a fixed identity and associated loader instance.
    pub fn new(id: LoaderId, name: String, instance: Arc<dyn ModLoader>) -> Self {
        Self { id, name, instance }
    }

    /// Returns the stable identifier used as the key in loader maps.
    pub fn id(&self) -> &LoaderId {
        &self.id
    }

    /// Returns the loader's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a shared reference to the associated loader instance.
    pub fn instance(&self) -> &Arc<dyn ModLoader> {
        &self.instance
    }
}
