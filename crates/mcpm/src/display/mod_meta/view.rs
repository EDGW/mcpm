//! Borrowed display views for runtime, cached and selected Mod declarations.

use mcpm_backend::local::{
    cache::mods::meta::CachedIdentification,
    mcmod::{CachedModMeta, ModInfo, ModMeta},
};

pub(crate) struct Declaration<'a> {
    pub info: &'a ModInfo,
    pub source: Vec<&'a str>,
}

/// Keeps unchecked results distinct from successful cached non-recognition.
pub(super) enum Content<'a> {
    Missing,
    Unrecognized,
    Recognized(Vec<Declaration<'a>>),
}

pub(crate) struct View<'a> {
    pub(super) loader: &'a str,
    pub(super) loader_name: &'a str,
    pub(super) content: Content<'a>,
}

impl<'a> View<'a> {
    /// Adapts selected declarations received by mod info, including over Remote.
    pub(crate) fn declarations(
        loader: &'a str,
        loader_name: &'a str,
        declarations: impl IntoIterator<Item = Declaration<'a>>,
    ) -> Self {
        let records: Vec<_> = declarations.into_iter().collect();
        Self {
            loader,
            loader_name,
            content: if records.is_empty() {
                Content::Missing
            } else {
                Content::Recognized(records)
            },
        }
    }

    /// Uses the cache owner's typed result rather than interpreting storage nulls.
    pub(crate) fn cached(loader: &'a str, result: CachedIdentification<'a>) -> Self {
        match result {
            CachedIdentification::Recognized(meta) => Self::from(meta),
            CachedIdentification::Unrecognized => Self {
                loader,
                loader_name: loader,
                content: Content::Unrecognized,
            },
            CachedIdentification::Missing => Self {
                loader,
                loader_name: loader,
                content: Content::Missing,
            },
        }
    }
}

impl<'a> From<&'a ModMeta> for View<'a> {
    fn from(meta: &'a ModMeta) -> Self {
        let mut records = Vec::new();
        append(Tree::Runtime(meta), &mut Vec::new(), &mut records);
        Self::declarations(meta.loader.id().as_str(), meta.loader.name(), records)
    }
}

impl<'a> From<&'a CachedModMeta> for View<'a> {
    fn from(meta: &'a CachedModMeta) -> Self {
        let mut records = Vec::new();
        append(Tree::Cached(meta), &mut Vec::new(), &mut records);
        Self {
            loader: &meta.loader,
            loader_name: &meta.loader,
            content: Content::Recognized(records),
        }
    }
}

enum Tree<'a> {
    Runtime(&'a ModMeta),
    Cached(&'a CachedModMeta),
}

fn append<'a>(tree: Tree<'a>, source: &mut Vec<&'a str>, records: &mut Vec<Declaration<'a>>) {
    let mods = match tree {
        Tree::Runtime(meta) => &meta.mods,
        Tree::Cached(meta) => &meta.mods,
    };
    records.extend(mods.iter().map(|info| Declaration {
        info,
        source: source.clone(),
    }));
    match tree {
        Tree::Runtime(meta) => {
            for child in &meta.nested {
                source.push(&child.path);
                append(Tree::Runtime(&child.meta), source, records);
                source.pop();
            }
        }
        Tree::Cached(meta) => {
            for child in &meta.nested {
                source.push(&child.path);
                append(Tree::Cached(&child.meta), source, records);
                source.pop();
            }
        }
    }
}
