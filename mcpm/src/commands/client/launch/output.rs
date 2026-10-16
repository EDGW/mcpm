use std::{
    io::{self, Write},
    process::ExitStatus,
};

use super::super::output::text;
use crate::{commands::output_style::OutputStyle, terminal};

pub(super) enum Event {
    Preparing,
    Started { pid: u32 },
    Exited { status: ExitStatus },
}

/// Remember output failures while still reaping every child already launched.
/// A broken stdout must not cause an early return that detaches running games.
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
        match event {
            Event::Preparing => writeln!(
                output,
                "{} {}",
                style.theme("Preparing"),
                style.important(name)
            ),
            Event::Started { pid } => writeln!(
                output,
                "{} {}  {} {}",
                style.required("Started"),
                style.important(name),
                style.secondary("PID"),
                style.important(pid)
            ),
            Event::Exited { status } => writeln!(
                output,
                "{} {}  {}",
                if status.success() {
                    style.required("Exited")
                } else {
                    style.danger("Exited")
                },
                style.important(name),
                style.secondary(status)
            ),
        }?;
    }
    output.flush()
}
