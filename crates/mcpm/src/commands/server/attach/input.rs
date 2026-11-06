//! Cancellable terminal-key input and bounded line input; neither owns the Java console.

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEvent};
use std::{
    io::{self, BufRead, Read},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

pub(super) enum Input {
    Line(String),
    Key(KeyEvent),
    Eof,
    Error(anyhow::Error),
}
pub(super) struct Reader {
    stop: Arc<AtomicBool>,
    keyboard: Arc<Keyboard>,
    pub events: mpsc::Receiver<Input>,
}
impl Reader {
    pub fn new(tui: bool) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let keyboard = Arc::new(Keyboard {
            gate: Mutex::new(()),
            pause_requested: AtomicBool::new(false),
            resumed: Condvar::new(),
        });
        let reader_keyboard = keyboard.clone();
        let (sender, events) = mpsc::channel(32);
        std::thread::Builder::new()
            .name("server-attach-input".into())
            .spawn(move || {
                let result = if tui {
                    keys(&sender, &cancelled, &reader_keyboard)
                } else {
                    lines(&sender, &cancelled)
                };
                if let Err(error) = result {
                    let _ = sender.blocking_send(Input::Error(error));
                }
            })
            .context("failed to create attach input reader")?;
        Ok(Self {
            stop,
            events,
            keyboard,
        })
    }
    pub(super) fn pause(&self) -> Paused<'_> {
        self.keyboard.pause_requested.store(true, Ordering::Release);
        Paused {
            _guard: self.keyboard.gate.lock().unwrap(),
            keyboard: &self.keyboard,
        }
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Keyboard {
    gate: Mutex<()>,
    pause_requested: AtomicBool,
    resumed: Condvar,
}
pub(super) struct Paused<'a> {
    _guard: MutexGuard<'a, ()>,
    keyboard: &'a Keyboard,
}
impl Drop for Paused<'_> {
    fn drop(&mut self) {
        self.keyboard
            .pause_requested
            .store(false, Ordering::Release);
        self.keyboard.resumed.notify_all();
    }
}
fn keys(sender: &mpsc::Sender<Input>, stop: &AtomicBool, keyboard: &Keyboard) -> Result<()> {
    while !stop.load(Ordering::Relaxed) {
        let mut guard = keyboard.gate.lock().unwrap();
        while keyboard.pause_requested.load(Ordering::Acquire) {
            guard = keyboard.resumed.wait(guard).unwrap();
        }
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            drop(guard);
            if sender.blocking_send(Input::Key(key)).is_err() {
                break;
            }
        }
    }
    Ok(())
}
fn lines(sender: &mpsc::Sender<Input>, stop: &AtomicBool) -> Result<()> {
    let mut input = io::stdin().lock();
    while !stop.load(Ordering::Relaxed) {
        let mut bytes = Vec::new();
        let length = input
            .by_ref()
            .take(16 * 1024 + 2)
            .read_until(b'\n', &mut bytes)?;
        if length == 0 {
            let _ = sender.blocking_send(Input::Eof);
            break;
        }
        anyhow::ensure!(length <= 16 * 1024 + 1, "Minecraft command exceeds 16 KiB");
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let line = String::from_utf8(bytes).context("Minecraft command input is not UTF-8")?;
        if sender.blocking_send(Input::Line(line)).is_err() {
            break;
        }
    }
    Ok(())
}
