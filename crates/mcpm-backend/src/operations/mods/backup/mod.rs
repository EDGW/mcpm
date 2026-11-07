//! Host backup workflows over the existing backup library and cache policies.

mod model;
pub use model::*;
mod create;
pub use create::{Create, CreateInput, CreateStorage};
mod apply;
pub use apply::{Apply, ApplyInput};
mod verify;
pub use verify::{Verify, VerifyEvent, VerifyInput};
mod checksum;
pub use checksum::{Checksum, ChecksumEvent, ChecksumInput};
mod verify_with;
pub use verify_with::{Target, VerifyWith, VerifyWithEvent, VerifyWithInput};
mod delete;
pub use delete::{Delete, DeleteEvent, DeleteInput};

use mcpm_lib::local::cache::mods::ModsCacheSelection;
use std::path::PathBuf;

fn cache_selection(directory: Option<PathBuf>) -> ModsCacheSelection {
    directory
        .map(|path| ModsCacheSelection::Directory { path })
        .unwrap_or(ModsCacheSelection::Default)
}
