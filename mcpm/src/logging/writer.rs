use std::{
    io::{self, Write},
    sync::Mutex,
};

use indicatif::MultiProgress;
use tracing_subscriber::fmt::MakeWriter;

static PROGRESS: Mutex<Option<MultiProgress>> = Mutex::new(None);

/// Temporarily associates logging with the active progress display. Previous
/// context is restored on drop, including output-error and early-return paths.
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

pub(super) struct WriterFactory;
pub(super) struct LogWriter;
impl<'a> MakeWriter<'a> for WriterFactory {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LogWriter
    }
}

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
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
