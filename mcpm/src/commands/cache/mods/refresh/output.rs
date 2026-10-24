//! Refresh presentation selection and shared per-entry success/failure handling.

use std::io::Write;

use anyhow::{Context, Result};
use mcpm_backend::{
    local::cache::{
        hash::Blake3Hash,
        mods::{ModsCacheDir, ident::RefreshStatus, meta::CacheEntry},
    },
    source::SourceId,
};

use super::super::entry;
use crate::{
    display::{
        cache::{Entry, refresh},
        console::ConsoleWriter,
    },
    terminal,
};

const SHORT_THRESHOLD: usize = 3;

pub(super) struct Presentation<'a> {
    out: &'a mut dyn Write,
    cache: &'a ModsCacheDir,
    mode: Mode,
    written: bool,
}

impl<'a> Presentation<'a> {
    pub(super) fn new(
        out: &'a mut dyn Write,
        cache: &'a ModsCacheDir,
        count: usize,
        short: bool,
        details: bool,
    ) -> Self {
        Self {
            out,
            cache,
            mode: if !terminal::stdout_is_tty() {
                Mode::Raw
            } else if short || (!details && count > SHORT_THRESHOLD) {
                Mode::Short
            } else {
                Mode::Details
            },
            written: false,
        }
    }

    /// Failed short rows contain only a result; the original error reaches diagnostics.
    pub(super) fn check<T>(&mut self, id: Blake3Hash, result: Result<T>) -> Result<T> {
        if result.is_err() && matches!(self.mode, Mode::Short) {
            // Preserve the operation's error even if stdout has also become unavailable.
            let _ = refresh::short(&mut ConsoleWriter::new(self.out), id, false);
        }
        result
    }

    pub(super) fn meta(&mut self, entry: &CacheEntry) -> Result<()> {
        self.write(entry.id(), |out, content| {
            refresh::entry(out, content, entry)
        })
    }

    pub(super) fn ident(
        &mut self,
        id: Blake3Hash,
        results: &[(SourceId, RefreshStatus)],
    ) -> Result<()> {
        let cache = self.cache;
        self.write(id, |out, content| {
            let info = cache.try_ident_info(id).with_context(|| {
                format!("failed to observe refreshed source identification for {id}")
            })?;
            refresh::ident_info(out, content, &info, results)
        })
    }

    fn write(
        &mut self,
        id: Blake3Hash,
        details: impl FnOnce(&mut ConsoleWriter<'_>, &Entry) -> Result<()>,
    ) -> Result<()> {
        match self.mode {
            Mode::Raw => Ok(()),
            Mode::Short => refresh::short(&mut ConsoleWriter::new(self.out), id, true),
            Mode::Details => {
                let inspection = self
                    .cache
                    .inspect(&id)
                    .with_context(|| format!("failed to inspect refreshed cache content {id}"))?;
                let content =
                    entry::observe(self.cache, id, Ok(inspection), &refresh::COLUMNS, false)?;
                let mut out = ConsoleWriter::new(self.out);
                if self.written {
                    writeln!(out)?;
                }
                details(&mut out, &content)?;
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
