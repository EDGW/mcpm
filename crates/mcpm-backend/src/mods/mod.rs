//! Mod reading, selection and digest operations over one host's files.
//!
//! Every operation takes a [`types::ModsScope`] so tools and server commands share the
//! same implementation; the server's mods directory and loader are resolved on the host.
//! Presentation filtering and the TUI live in `mcpm`.

pub mod digest;
pub mod inputs;
pub mod read;
pub mod records;
pub mod scan;
pub mod select;
pub mod selection;
pub mod types;

pub use read::{JarRead, ReadEvent, ReadInput, ReadKind};
pub use select::{Select, SelectInput, SelectKind, SelectSummary};
pub use types::{ModsCachePolicy, ModsScope};

/// Default ceiling for file-host parallelism.
pub const DEFAULT_PARALLELISM: usize = 10;
