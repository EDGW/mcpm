//! Progress-aware stderr writer for terminal logs; file logging is independent of terminal drawing.

use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::Mutex,
};

use indicatif::MultiProgress;
use tracing_subscriber::fmt::MakeWriter;

static CONSOLE: Mutex<Console> = Mutex::new(Console {
    depth: 0,
    pending: VecDeque::new(),
    dropped: 0,
    channel: None,
});

#[derive(Debug)]
struct Console {
    depth: usize,
    pending: VecDeque<u8>,
    dropped: usize,
    channel: Option<tokio::sync::mpsc::Sender<(Vec<u8>, usize)>>,
}

const MAX_PENDING: usize = 256 * 1024;

/// Delay terminal diagnostics while a full-screen interface owns the terminal.
#[derive(Debug)]
pub(crate) struct ConsoleGuard;

pub(crate) fn pause_console() -> ConsoleGuard {
    CONSOLE.lock().unwrap().depth += 1;
    ConsoleGuard
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        let (mut pending, mut dropped) = {
            let mut console = CONSOLE.lock().unwrap();
            console.depth -= 1;
            if console.depth != 0 {
                return;
            }
            (
                std::mem::take(&mut console.pending),
                std::mem::take(&mut console.dropped),
            )
        };
        if dropped > 0 {
            // Do not resume the terminal in the middle of a formatted log/escape sequence.
            while let Some(byte) = pending.pop_front() {
                dropped += 1;
                if byte == b'\n' {
                    break;
                }
            }
            let _ = writeln!(
                LogWriter,
                "[diagnostics] {dropped} terminal log bytes omitted while TUI was active"
            );
        }
        let (first, second) = pending.as_slices();
        let _ = LogWriter.write_all(first);
        let _ = LogWriter.write_all(second);
    }
}

/// Routes terminal diagnostics to an inline renderer; the independent file sink is unchanged.
#[derive(Debug)]
pub(crate) struct ConsoleChannelGuard {
    previous: Option<tokio::sync::mpsc::Sender<(Vec<u8>, usize)>>,
}
pub(crate) fn redirect_console() -> (
    ConsoleChannelGuard,
    tokio::sync::mpsc::Receiver<(Vec<u8>, usize)>,
) {
    let (sender, receiver) = tokio::sync::mpsc::channel(32);
    let previous = CONSOLE.lock().unwrap().channel.replace(sender);
    (ConsoleChannelGuard { previous }, receiver)
}
impl Drop for ConsoleChannelGuard {
    fn drop(&mut self) {
        let dropped = {
            let mut console = CONSOLE.lock().unwrap();
            console.channel = self.previous.take();
            std::mem::take(&mut console.dropped)
        };
        if dropped > 0 {
            let _ = writeln!(
                LogWriter,
                "[diagnostics] {dropped} terminal log bytes omitted while inline output was busy"
            );
        }
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
                if bytes.len() >= MAX_PENDING {
                    console.dropped += console.pending.len() + bytes.len() - MAX_PENDING;
                    console.pending.clear();
                    console.pending.extend(&bytes[bytes.len() - MAX_PENDING..]);
                } else {
                    let remove = (console.pending.len() + bytes.len()).saturating_sub(MAX_PENDING);
                    console.pending.drain(..remove);
                    console.dropped += remove;
                    console.pending.extend(bytes);
                }
                return Ok(bytes.len());
            }
            if let Some(channel) = console.channel.clone() {
                for chunk in bytes.chunks(8192) {
                    let dropped = console.dropped;
                    if channel.try_send((chunk.to_vec(), dropped)).is_ok() {
                        console.dropped = 0;
                    } else {
                        console.dropped += chunk.len();
                    }
                }
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
