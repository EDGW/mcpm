//! Cancellable network waits that keep terminal events and repainting responsive.

use std::{
    future::Future,
    io,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::runtime::Runtime;

/// Dropping the request on Escape or interruption cancels its remaining work.
/// Ctrl-C propagates through nested browsers so every screen guard is restored.
pub(super) fn wait<T>(
    runtime: &Runtime,
    request: impl Future<Output = Result<T>>,
    label: &str,
    mut render: impl FnMut(&str) -> io::Result<()>,
) -> Result<Option<T>> {
    runtime.block_on(async {
        tokio::pin!(request);
        let mut tick = tokio::time::interval(Duration::from_millis(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let started = Instant::now();
        let mut last_frame = None;
        loop {
            tokio::select! {
                biased;
                _ = tick.tick() => {
                    // Bound each batch so held keys cannot starve network I/O.
                    for _ in 0..32 {
                        if !event::poll(Duration::ZERO)? { break; }
                        match event::read()? {
                            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                    return Err(io::Error::new(io::ErrorKind::Interrupted, "request interrupted").into());
                                }
                                if key.code == KeyCode::Esc { return Ok(None); }
                            }
                            Event::Resize(_, _) => last_frame = None,
                            _ => {}
                        }
                    }
                    let frame = started.elapsed().as_millis() / 100;
                    if last_frame != Some(frame) {
                        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                        let status = format!("{} {label} {:.1}s · Esc cancel · Ctrl-C exit",
                            frames[(frame % frames.len() as u128) as usize], started.elapsed().as_secs_f32());
                        render(&status)?;
                        last_frame = Some(frame);
                    }
                }
                result = &mut request => return result.map(Some),
            }
        }
    })
}

pub(super) fn is_interrupted(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::Interrupted)
}
