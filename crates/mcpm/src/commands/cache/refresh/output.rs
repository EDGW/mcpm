//! Local refresh rendering over Host-provided observations; Raw success remains silent.

use crate::{
    display::{
        cache::{Entry, refresh},
        console::ConsoleWriter,
    },
    terminal,
};
use anyhow::Result;
use mcpm_backend::operations::cache::refresh::{IdentReceipt, MetaReceipt};
use mcpm_lib::local::cache::{hash::Blake3Hash, mods::ident::RefreshStatus};
use std::io::Write;

const SHORT_THRESHOLD: usize = 3;
pub(super) struct Presentation<'a> {
    out: &'a mut dyn Write,
    mode: Mode,
    written: bool,
    short: bool,
    details: bool,
}
impl<'a> Presentation<'a> {
    pub(super) fn new(out: &'a mut dyn Write, short: bool, details: bool) -> Self {
        Self {
            out,
            mode: Mode::Raw,
            written: false,
            short,
            details,
        }
    }
    pub(super) fn selected(&mut self, count: usize) {
        self.mode = if !terminal::stdout_is_tty() {
            Mode::Raw
        } else if self.short || (!self.details && count > SHORT_THRESHOLD) {
            Mode::Short
        } else {
            Mode::Details
        };
    }
    pub(super) fn check<T>(&mut self, id: Blake3Hash, result: Result<T>) -> Result<T> {
        if result.is_err() && matches!(self.mode, Mode::Short) {
            let _ = refresh::short(&mut ConsoleWriter::new(self.out), id, false);
        }
        result
    }
    pub(super) fn meta(&mut self, entry: &MetaReceipt) -> Result<()> {
        self.write(&entry.content, |out| {
            refresh::entry(out, &entry.content, entry)
        })
    }
    pub(super) fn ident(&mut self, id: Blake3Hash, entry: &IdentReceipt) -> Result<()> {
        match self.mode {
            Mode::Raw => Ok(()),
            Mode::Short => {
                let mut out = ConsoleWriter::new(self.out);
                writeln!(out, "{id}  {}", entry.overall_status().as_str())?;
                Ok(())
            }
            Mode::Details => {
                let mut out = ConsoleWriter::new(self.out);
                if self.written {
                    writeln!(out)?;
                }
                writeln!(
                    out,
                    "Identification: {}",
                    entry.identification.status().as_str()
                )?;
                if entry.overall_status() != entry.identification.status() {
                    writeln!(out, "Overall result: {}", entry.overall_status().as_str())?;
                }
                if let Ok(observation) = &entry.observation {
                    let results: Vec<_> = entry
                        .identification
                        .sources
                        .iter()
                        .filter_map(|source| {
                            use mcpm_backend::operations::mods::identification::SourceResolution;
                            let status = match source.resolution {
                                SourceResolution::Cached(_) => RefreshStatus::Reused,
                                SourceResolution::Queried(Some(_)) => RefreshStatus::Matched,
                                SourceResolution::Queried(None) => RefreshStatus::Unmatched,
                                SourceResolution::Failed(_) | SourceResolution::Skipped => {
                                    return None;
                                }
                            };
                            Some((source.source.clone(), status))
                        })
                        .collect();
                    refresh::ident_info(
                        &mut out,
                        &observation.content,
                        &observation.info,
                        &observation.metadata_path,
                        &results,
                    )?;
                } else {
                    writeln!(out, "Stored snapshot: unavailable")?;
                }
                self.written = true;
                Ok(())
            }
        }
    }
    pub(super) fn failed(&mut self, id: Blake3Hash) -> Result<()> {
        if matches!(self.mode, Mode::Short) {
            refresh::short(&mut ConsoleWriter::new(self.out), id, false)?;
        }
        Ok(())
    }
    fn write(
        &mut self,
        content: &Entry,
        details: impl FnOnce(&mut ConsoleWriter<'_>) -> Result<()>,
    ) -> Result<()> {
        match self.mode {
            Mode::Raw => Ok(()),
            Mode::Short => refresh::short(&mut ConsoleWriter::new(self.out), content.id, true),
            Mode::Details => {
                let mut out = ConsoleWriter::new(self.out);
                if self.written {
                    writeln!(out)?;
                }
                details(&mut out)?;
                self.written = true;
                Ok(())
            }
        }
    }
}
enum Mode {
    Raw,
    Short,
    Details,
}
