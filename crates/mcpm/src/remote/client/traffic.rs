//! Count actual protocol bytes written to child stdin and read from child stdout.

use std::{
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Default)]
pub(super) struct Traffic {
    stdin: Arc<AtomicU64>,
    stdout: Arc<AtomicU64>,
}

impl Traffic {
    pub(super) fn stdin<T>(&self, pipe: T) -> Metered<T> {
        Metered {
            pipe,
            bytes: self.stdin.clone(),
        }
    }

    pub(super) fn stdout<T>(&self, pipe: T) -> Metered<T> {
        Metered {
            pipe,
            bytes: self.stdout.clone(),
        }
    }

    pub(super) fn totals(&self) -> (u64, u64) {
        (
            self.stdin.load(Ordering::Relaxed),
            self.stdout.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug)]
pub(super) struct Metered<T> {
    pipe: T,
    bytes: Arc<AtomicU64>,
}

impl<T: Read> Read for Metered<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.pipe.read(buffer)?;
        self.bytes.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}

impl<T: Write> Write for Metered<T> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.pipe.write(buffer)?;
        self.bytes.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.pipe.flush()
    }
}
