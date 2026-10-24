//! Shared CLI support for mod inspection, independent of command input sources.

pub(in crate::commands) mod cache;
pub(in crate::commands) mod dep_graph;
pub(in crate::commands) mod ignored_output;
pub(in crate::commands) mod inspect;
pub(in crate::commands) mod list;
pub(in crate::commands) mod loaders;
pub(in crate::commands) mod mod_records;
pub(in crate::commands) mod reuse;
pub(in crate::commands) mod scan;
pub(in crate::commands) mod scan_output;
pub(in crate::commands) mod selection;
pub(in crate::commands) mod source_path;
