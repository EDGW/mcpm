//! Explicit Mod cache initialization through the library-owned directory policy.

use anyhow::Result;
use mcpm_backend::local::cache::mods::{ModsCacheDir, ModsCacheSelection};
use std::io::Write;

pub(super) fn run(
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let cache = ModsCacheDir::initialize(selection.path()?)?;
    crate::display::cache::operations::initialized(out, cache.path(), headers)
}
