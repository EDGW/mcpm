//! Unfiltered English cache-dependency notices, kept off Raw stdout and outside progress stages.

use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Result;

use crate::display::{
    console::ConsoleWriter,
    style::{OutputStyle, text},
};

pub(crate) fn cache_only_warning() -> Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(ConsoleWriter::new(&mut stderr), "{}", OutputStyle::stderr().warning(
        "Warning: --cache-only saves only a manifest. Any referenced file contents are required in cache for restoration. Cache cleanup, loss or corruption may make restoration impossible. Copying the manifest does not copy those contents."
    ))?;
    Ok(())
}

pub(crate) fn cache_only_created(cache: &Path, empty: bool) -> Result<()> {
    let mut stderr = io::stderr().lock();
    let mut console = ConsoleWriter::new(&mut stderr);
    let style = OutputStyle::stderr();
    writeln!(
        console,
        "Manifest-only backup created. Cache: {}",
        style.muted(text(&cache.to_string_lossy()))
    )?;
    if empty {
        writeln!(
            console,
            "The manifest is empty; no cached file contents are required for restoration."
        )?;
    } else {
        writeln!(console, "{}", style.warning("Keep the referenced cache contents to restore this backup. Copying the manifest alone is not a complete backup."))?;
    }
    Ok(())
}
