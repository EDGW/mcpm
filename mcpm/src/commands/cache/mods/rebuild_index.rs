//! Explicit rebuilding of the generic derived content lookup index.

use anyhow::Result;
use kako_mods_lib::local::cache::mods::ModsCacheSelection;
use std::io::Write;

pub(super) fn run(
    selection: ModsCacheSelection,
    out: &mut impl Write,
    headers: bool,
) -> Result<()> {
    let cache = selection.open()?;
    crate::display::cache::operations::rebuilt_index(out, cache.rebuild_index()?, headers)
}
