//! Ordered file digest field groups and fixed-column Raw TSV presentation.

use std::{io::Write, path::Path};

use anyhow::Result;
use mcpm_lib::source::FileSummary;

use super::{
    console::ConsoleWriter,
    size::size,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

pub(crate) struct Presentation<W: Write> {
    output: W,
    murmur2: bool,
    headers: bool,
    printed: bool,
}

impl<W: Write> Presentation<W> {
    pub(crate) fn new(output: W, murmur2: bool, headers: bool) -> Self {
        Self {
            output,
            murmur2,
            headers,
            printed: false,
        }
    }

    pub(crate) fn file(&mut self, path: &Path, summary: Option<&FileSummary>) -> Result<()> {
        if terminal::stdout_is_tty() {
            let style = OutputStyle::new();
            let mut console = ConsoleWriter::new(&mut self.output);
            if self.printed {
                writeln!(console)?;
            }
            writeln!(
                console,
                "File: {}",
                style.muted(text(&path.to_string_lossy()))
            )?;
            let mut fields = console.subwriter();
            if let Some(summary) = summary {
                writeln!(fields, "Size: {}", size(u128::from(summary.size), &style))?;
                writeln!(fields, "SHA-1: {}", style.primary(&summary.sha1))?;
                if self.murmur2
                    && let Some(fingerprint) = summary.murmur2
                {
                    writeln!(fields, "Murmur2: {}", style.primary(fingerprint))?;
                }
            } else {
                writeln!(fields, "{}", style.danger("Digest could not be computed."))?;
            }
        } else {
            if self.headers && !self.printed {
                writeln!(self.output, "file\tsize\tsha1\tmurmur2")?;
            }
            write!(self.output, "{}\t", tsv(&path.to_string_lossy()))?;
            if let Some(summary) = summary {
                write!(self.output, "{}\t{}\t", summary.size, summary.sha1)?;
                if self.murmur2
                    && let Some(fingerprint) = summary.murmur2
                {
                    write!(self.output, "{fingerprint}")?;
                }
                writeln!(self.output)?;
            } else {
                writeln!(self.output, "\t\t")?;
            }
        }
        self.printed = true;
        self.output.flush()?;
        Ok(())
    }
}
