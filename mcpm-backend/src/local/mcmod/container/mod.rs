//! Mod archive container interfaces.

use anyhow::Result;

use crate::local::{
    mcmod::{ModDirectory, ModEntry},
    modloader::LoaderInfo,
};

#[derive(Debug, Clone)]
pub struct ModContainer {
    loader: LoaderInfo,
    dir: ModDirectory,
}

impl ModContainer {
    pub fn new(loader: LoaderInfo, dir: ModDirectory) -> Self {
        Self { loader, dir }
    }

    pub fn loader(&self) -> &LoaderInfo {
        &self.loader
    }

    pub fn directory(&self) -> &ModDirectory {
        &self.dir
    }

    pub fn mods(&self) -> Result<Vec<ModEntry>> {
        self.dir.entries()
    }
}
