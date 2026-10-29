//! Read-only cache presentation data, with content and named metadata observed independently.

use std::path::PathBuf;

use mcpm_backend::local::cache::{
    hash::Blake3Hash,
    mods::{ident::CacheIdentInfo, meta::CacheEntry},
};

pub(crate) struct Entry {
    pub id: Blake3Hash,
    pub file: PathBuf,
    pub status: &'static str,
    pub reason: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub metadatas: Vec<Metadata>,
}

pub(crate) enum Metadata {
    Meta(CacheEntry),
    Ident(CacheIdentInfo),
    Other(String),
}

impl Metadata {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Meta(_) => mcpm_backend::local::cache::mods::meta::NAME,
            Self::Ident(_) => mcpm_backend::local::cache::mods::ident::NAME,
            Self::Other(name) => name,
        }
    }
}
