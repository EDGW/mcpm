//! Cache management warnings and tips, separate from standard error diagnostics.

use std::io::{self, Write};

use anyhow::Result;

use super::super::{console::ConsoleWriter, style::OutputStyle};

pub(crate) fn stash_protection_notice() -> Result<()> {
    let mut stderr = io::stderr().lock();
    let mut output = ConsoleWriter::new(&mut stderr);
    let style = OutputStyle::stderr();
    writeln!(
        output,
        "{} Do not manage server stash caches manually.",
        style.warning("Warning:")
    )?;
    writeln!(
        output.subwriter(),
        "Stashes depend on their contents; changes {}.",
        style.warning("may make stashes unusable")
    )?;
    writeln!(
        output,
        "Tip: Use {} to bypass this protection.",
        style.primary("--no-protect-stash")
    )?;
    output.flush()?;
    Ok(())
}
