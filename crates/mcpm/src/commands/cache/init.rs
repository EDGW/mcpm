//! Explicit cache initialization on the selected execution host.

use anyhow::Result;
use mcpm_backend::operations::cache::Init;
use std::io::Write;

pub(super) async fn run(context: &super::Context, out: &mut impl Write) -> Result<()> {
    let result = context
        .executor()
        .await?
        .call::<Init>(context.cache.clone())
        .await?;
    crate::display::cache::operations::initialized(
        out,
        &result.directory,
        result.version,
        context.headers,
    )
}
