//! Progress-aware stderr writer for terminal logs; file logging is independent of terminal drawing.

use std::{
    io::{self, Write},
    sync::Mutex,
};

use indicatif::MultiProgress;
use tracing_subscriber::fmt::MakeWriter;

static CONSOLE: Mutex<Console> = Mutex::new(Console {
    depth: 0,
    pending: Vec::new(),
});

#[derive(Debug)]
struct Console {
    depth: usize,
    pending: Vec<u8>,
}

/// Delay terminal diagnostics while a full-screen interface owns the terminal.
#[derive(Debug)]
pub(crate) struct ConsoleGuard;

pub(crate) fn pause_console() -> ConsoleGuard {
    CONSOLE.lock().unwrap().depth += 1;
    ConsoleGuard
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        let pending = {
            let mut console = CONSOLE.lock().unwrap();
            console.depth -= 1;
            if console.depth != 0 {
                return;
            }
            std::mem::take(&mut console.pending)
        };
        let _ = LogWriter.write_all(&pending);
    }
}

static PROGRESS: Mutex<Option<MultiProgress>> = Mutex::new(None);

/// Temporarily associates logging with the active progress display. Previous
/// context is restored on drop, including output-error and early-return paths.
#[derive(Debug)]
pub(crate) struct ProgressGuard {
    previous: Option<MultiProgress>,
}

pub(crate) fn attach_progress(display: Option<MultiProgress>) -> ProgressGuard {
    let mut current = PROGRESS.lock().unwrap_or_else(|error| error.into_inner());
    ProgressGuard {
        previous: std::mem::replace(&mut *current, display),
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        *PROGRESS.lock().unwrap_or_else(|error| error.into_inner()) = self.previous.take();
    }
}

#[derive(Debug)]
pub(super) struct WriterFactory;
#[derive(Debug)]
pub(super) struct LogWriter;
impl<'a> MakeWriter<'a> for WriterFactory {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LogWriter
    }
}

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        {
            let mut console = CONSOLE.lock().unwrap();
            if console.depth > 0 {
                console.pending.extend_from_slice(bytes);
                return Ok(bytes.len());
            }
        }
        // Release our lock before calling indicatif; it serializes draw/writes.
        let display = PROGRESS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let write = || io::stderr().lock().write(bytes);
        if let Some(display) = display {
            display.suspend(write)
        } else {
            write()
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stderr().lock().flush()
    }
}
