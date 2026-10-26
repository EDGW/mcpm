//! Adapts process notifications into typed backend stop requests without owning Java PIDs.

use anyhow::{Context, Result};
use mcpm_backend::local::mc::server::launch::StopRequest;
use std::sync::mpsc::Sender;

#[cfg(unix)]
pub(super) struct Handler {
    handle: signal_hook::iterator::Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl Handler {
    pub(super) fn install(sender: Sender<StopRequest>) -> Result<Self> {
        use signal_hook::{
            consts::{SIGHUP, SIGINT, SIGTERM},
            iterator::Signals,
        };
        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP])
            .context("failed to register foreground stop notifications")?;
        let handle = signals.handle();
        let thread = std::thread::Builder::new()
            .name("server-launch-signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    if sender
                        .send(if signal == SIGINT {
                            StopRequest::Interrupt
                        } else {
                            StopRequest::Terminate
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .context("failed to start foreground signal adapter")?;
        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }
}

#[cfg(unix)]
impl Drop for Handler {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
pub(super) struct Handler;

#[cfg(windows)]
impl Handler {
    pub(super) fn install(sender: Sender<StopRequest>) -> Result<Self> {
        ctrlc::set_handler(move || {
            let _ = sender.send(StopRequest::ConsoleInterrupt);
        })
        .context("failed to register foreground Ctrl+C notifications")?;
        Ok(Self)
    }
}
