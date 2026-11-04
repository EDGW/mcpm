//! Console rendering for host-projected manifest files and workspace differences.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::operations::server::stash::{Difference, FileRecord};

use crate::display::{
    console::ConsoleWriter,
    size::size,
    style::{OutputStyle, text},
};

pub(super) fn file(out: &mut ConsoleWriter<'_>, record: &FileRecord) -> Result<()> {
    let style = OutputStyle::new();
    writeln!(out, "File: {}", style.primary(text(&record.name)))?;
    let mut fields = out.subwriter();
    writeln!(fields, "BLAKE3: {}", style.muted(&record.blake3))?;
    writeln!(fields, "Size: {}", size(u128::from(record.size), &style))?;
    Ok(())
}

pub(super) fn difference(out: &mut ConsoleWriter<'_>, difference: &Difference) -> Result<()> {
    let style = OutputStyle::new();
    match difference {
        Difference::Missing { expected } => {
            writeln!(out, "Missing: {}", style.important(text(&expected.path)))?
        }
        Difference::Unexpected { actual } => {
            writeln!(out, "Unexpected: {}", style.important(text(&actual.path)))?
        }
        Difference::Changed { expected, actual } => {
            writeln!(out, "Changed: {}", style.important(text(&actual.path)))?;
            let mut fields = out.subwriter();
            if expected.size != actual.size {
                writeln!(
                    fields,
                    "Expected size: {}",
                    size(u128::from(expected.size), &style)
                )?;
                writeln!(
                    fields,
                    "Actual size: {}",
                    size(u128::from(actual.size), &style)
                )?;
            }
            if expected.blake3 != actual.blake3 {
                writeln!(fields, "Expected BLAKE3: {}", style.muted(&expected.blake3))?;
                writeln!(fields, "Actual BLAKE3: {}", style.muted(&actual.blake3))?;
            }
        }
    }
    Ok(())
}
