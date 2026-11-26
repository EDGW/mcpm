//! Console and Raw presentation for client launch output.

use std::{
    io::{self, Write},
    process::ExitStatus,
};

use mcpm_rich::rich;

use super::super::output::text;
use crate::{
    display::{rich::line, style::OutputStyle},
    terminal,
};

pub(super) enum Event {
    Preparing,
    Started { pid: u32 },
    Exited { status: ExitStatus },
}

/// Remember output failures while still reaping every child already launched.
/// A broken stdout must not cause an early return that detaches running games.
#[derive(Debug)]
pub(super) struct Reporter {
    error: Option<io::Error>,
}
impl Reporter {
    pub(super) fn new() -> Self {
        Self { error: None }
    }
    pub(super) fn event(&mut self, name: &str, event: Event) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = write(&mut io::stdout().lock(), name, event) {
            self.error = Some(error);
        }
    }
    pub(super) fn finish(self) -> io::Result<()> {
        self.error.map_or(Ok(()), Err)
    }
}

fn write(output: &mut impl Write, name: &str, event: Event) -> io::Result<()> {
    let name = text(name);
    if !terminal::stdout_is_tty() {
        match event {
            Event::Preparing => writeln!(output, "{name}\tpreparing\t"),
            Event::Started { pid } => writeln!(output, "{name}\tstarted\t{pid}"),
            Event::Exited { status } => writeln!(output, "{name}\texited\t{status}"),
        }?;
    } else {
        let style = OutputStyle::new();
        let message = match event {
            Event::Preparing => rich!("[Preparing {name}](primary)"),
            Event::Started { pid } => {
                rich!("[Started](success) [{name}](primary)  [PID](muted) [{pid}](important)")
            }
            Event::Exited { status } if status.success() => {
                rich!("[Exited](success) [{name}](primary)  [{status}](muted)")
            }
            Event::Exited { status } => {
                rich!("[Exited](danger) [{name}](primary)  [{status}](muted)")
            }
        };
        writeln!(output, "{}", line(&message, &style))?;
    }
    output.flush()
}
