//! Explicit rebuilding of the generic derived content lookup index.

use anyhow::Result;
use mcpm_backend::operations::cache::RebuildIndex;
use std::io::Write;

pub(super) async fn run(context: &super::Context, out: &mut impl Write) -> Result<()> {
    let indexed = context
        .executor()
        .await?
        .call::<RebuildIndex>(context.cache.clone())
        .await?;
    crate::display::cache::operations::rebuilt_index(out, indexed, context.headers)
}
