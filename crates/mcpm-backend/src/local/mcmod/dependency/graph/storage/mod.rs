//! Versioned graph JSON storage. Runtime loader instances come from the caller's registry.

pub mod error;
mod read;
mod write;

pub use read::read_json;
pub use write::write_json;

const FORMAT_VERSION: u64 = 1;
