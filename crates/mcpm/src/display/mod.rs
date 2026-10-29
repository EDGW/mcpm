//! Shared data rendering; command acquisition, transport and storage stay with their owners.

pub(crate) mod backup;
pub(crate) mod build_info;
pub(crate) mod cache;
pub(crate) mod console;
pub(crate) mod dependency;
pub(crate) mod error;
pub(crate) mod mod_meta;
pub(crate) mod size;
pub(crate) mod source_match;
pub(crate) mod stash;
pub(crate) mod style;

pub(crate) mod server;
