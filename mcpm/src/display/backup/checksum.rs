//! Directory checksum summaries and structured comparisons in Console and Raw modes.

use std::{io::Write, path::Path};

use anyhow::Result;
use mcpm_backend::local::mcmod::backup::{Comparison, DirectoryChecksum, FileDifference};

use crate::{
    display::{
        console::ConsoleWriter,
        size::size,
        style::{OutputStyle, text, tsv},
    },
    terminal,
};

pub(super) fn summary(
    out: &mut impl Write,
    path: &Path,
    computed: Option<&DirectoryChecksum>,
    headers: bool,
) -> Result<()> {
    let directory = computed.map(DirectoryChecksum::directory).unwrap_or(path);
    let status = if computed.is_some() { "ok" } else { "failed" };
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "\nDirectory: {}",
            style.muted(text(&directory.to_string_lossy()))
        )?;
        let mut fields = console.subwriter();
        let status = if computed.is_some() {
            style.success(status)
        } else {
            style.danger(status)
        };
        writeln!(fields, "Status: {status}")?;
        if let Some(computed) = computed {
            let total = computed
                .manifest()
                .files()
                .iter()
                .map(|file| u128::from(file.size()))
                .sum::<u128>();
            writeln!(
                fields,
                "Checksum: {}",
                style.primary(computed.manifest().checksum())
            )?;
            writeln!(fields, "Files: {}", computed.manifest().files().len())?;
            writeln!(fields, "Size: {}", size(total, &style))?;
        }
    } else {
        if headers {
            writeln!(out, "checksum")?;
        }
        if let Some(computed) = computed {
            writeln!(out, "{}", computed.manifest().checksum())?;
        }
    }
    Ok(())
}

pub(super) fn comparison(
    out: &mut impl Write,
    (kind, target): (&str, &str),
    path: &Path,
    computed: Option<&DirectoryChecksum>,
    comparison: Option<&Comparison>,
    headers: bool,
) -> Result<()> {
    let directory = computed.map(DirectoryChecksum::directory).unwrap_or(path);
    let status = match comparison {
        Some(comparison) if comparison.matches() => "match",
        Some(_) => "mismatch",
        None => "failed",
    };
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "\nTarget ({kind}): {}",
            style.secondary(text(target))
        )?;
        let mut fields = console.subwriter();
        writeln!(
            fields,
            "Directory: {}",
            style.muted(text(&directory.to_string_lossy()))
        )?;
        let status = if comparison.is_some_and(Comparison::matches) {
            style.success(status)
        } else {
            style.danger(status)
        };
        writeln!(fields, "Status: {status}")?;
        if let Some(comparison) = comparison {
            writeln!(
                fields,
                "Expected checksum: {}",
                style.muted(comparison.expected())
            )?;
            writeln!(
                fields,
                "Actual checksum: {}",
                style.primary(comparison.actual())
            )?;
            for difference in comparison.differences() {
                file_difference(&mut fields, difference)?;
            }
        } else if let Some(computed) = computed {
            writeln!(
                fields,
                "Actual checksum: {}",
                style.primary(computed.manifest().checksum())
            )?;
        }
    } else {
        if headers {
            writeln!(
                out,
                "target_type\ttarget\tdirectory\tstatus\texpected_checksum\tactual_checksum"
            )?;
        }
        writeln!(
            out,
            "{}\t{}\t{}\t{status}\t{}\t{}",
            tsv(kind),
            tsv(target),
            tsv(&directory.to_string_lossy()),
            comparison
                .map(|comparison| comparison.expected().to_string())
                .unwrap_or_default(),
            computed
                .map(|computed| computed.manifest().checksum().to_string())
                .unwrap_or_default()
        )?;
    }
    Ok(())
}

fn file_difference(out: &mut ConsoleWriter<'_>, difference: &FileDifference) -> Result<()> {
    let style = OutputStyle::new();
    match difference {
        FileDifference::Missing { expected } => {
            writeln!(out, "Missing: {}", style.important(text(expected.path())))?
        }
        FileDifference::Unexpected { actual } => {
            writeln!(out, "Unexpected: {}", style.important(text(actual.path())))?
        }
        FileDifference::Changed { expected, actual } => {
            writeln!(out, "Changed: {}", style.important(text(actual.path())))?;
            let mut fields = out.subwriter();
            if expected.size() != actual.size() {
                writeln!(
                    fields,
                    "Expected size: {}",
                    size(u128::from(expected.size()), &style)
                )?;
                writeln!(
                    fields,
                    "Actual size: {}",
                    size(u128::from(actual.size()), &style)
                )?;
            }
            if expected.blake3() != actual.blake3() {
                writeln!(
                    fields,
                    "Expected BLAKE3: {}",
                    style.muted(expected.blake3())
                )?;
                writeln!(fields, "Actual BLAKE3: {}", style.muted(actual.blake3()))?;
            }
        }
    }
    Ok(())
}
