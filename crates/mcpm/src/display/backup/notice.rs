//! Unfiltered English cache-dependency notices, kept off Raw stdout and outside progress stages.

use std::{io, path::Path};

use anyhow::Result;

use mcpm_rich::rich;

use crate::display::{console::ConsoleWriter, rich::block, style::OutputStyle};

pub(crate) fn cache_only_warning() -> Result<()> {
    let mut stderr = io::stderr().lock();
    let warning = rich!(
        "[Warning:](warning) `--cache-only` saves only a manifest. Any referenced file contents
         are required in cache for restoration. Cache cleanup, loss or corruption may make
         restoration impossible. Copying the manifest does not copy those contents."
    );
    block(
        &mut ConsoleWriter::new(&mut stderr),
        &warning,
        &OutputStyle::stderr(),
    )?;
    Ok(())
}

pub(crate) fn cache_only_created(cache: &Path, empty: bool) -> Result<()> {
    let mut stderr = io::stderr().lock();
    let mut console = ConsoleWriter::new(&mut stderr);
    let style = OutputStyle::stderr();
    let cache = cache.to_string_lossy();
    let mut notice = rich!("Manifest-only backup created. Cache: `{cache}`");
    notice.extend(if empty {
        rich!("The manifest is empty; no cached file contents are required for restoration.")
    } else {
        rich!(
            "[Keep the referenced cache contents to restore this backup. Copying the manifest
             alone is not a complete backup.](warning)"
        )
    });
    block(&mut console, &notice, &style)?;
    Ok(())
}
