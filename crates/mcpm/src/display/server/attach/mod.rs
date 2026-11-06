//! Inline/Console/Raw attach output; Minecraft prefix coloring is exclusively frontend presentation.

use crate::{
    logging, terminal,
    theme::{Role, widgets::style},
};
use anyhow::Result;
use mcpm_backend::operations::server::shim::{
    Channel, CommandOutcome, CommandResult, ServerStatus, Source, State,
};
use mcpm_exec::messaging::SessionTrafficStats;
use ratatui::text::{Line, Span};
use std::io::{self, Write};

mod inline;
mod log;
mod speed;
mod suggestions;
mod text;
pub(crate) use inline::modifier_name;

/// One inline terminal owner; all asynchronous terminal diagnostics are routed through its Channel.
pub(crate) struct Display {
    screen: Option<inline::Screen>,
    raw: bool,
    streams: [text::Stream; 3],
    formatting: [log::Formatting; 3],
    stderr_start: bool,
    pub draft: String,
    pub cursor: usize,
    pub state: State,
    pub channel: Channel,
    pub server_status: ServerStatus,
    suggestions: suggestions::Suggestions,
    speed_full: bool,
    pub diagnostics: tokio::sync::mpsc::Receiver<(Vec<u8>, usize)>,
    // Dropped after screen restoration, so terminal diagnostics cannot overwrite a live input area.
    _console: Option<logging::ConsoleChannelGuard>,
}
impl Display {
    pub fn new(
        no_ui: bool,
        height: u16,
        state: State,
        channel: Channel,
        server_status: ServerStatus,
    ) -> Result<Self> {
        let tui =
            !no_ui && terminal::stdout_is_tty() && terminal::stderr_is_tty() && !console::is_dumb();
        let (guard, diagnostics) = if tui {
            let (guard, receiver) = logging::redirect_console();
            (Some(guard), receiver)
        } else {
            let (_, receiver) = tokio::sync::mpsc::channel(1);
            (None, receiver)
        };
        Ok(Self {
            screen: if tui {
                Some(inline::Screen::new(height)?)
            } else {
                None
            },
            raw: !terminal::stdout_is_tty(),
            streams: Default::default(),
            formatting: Default::default(),
            stderr_start: true,
            draft: String::new(),
            cursor: 0,
            state,
            channel,
            server_status,
            suggestions: suggestions::Suggestions::default(),
            speed_full: false,
            diagnostics,
            _console: guard,
        })
    }
    pub fn is_tui(&self) -> bool {
        self.screen.is_some()
    }
    pub fn resize_input(&mut self, grow: bool) -> Result<()> {
        if let Some(screen) = &mut self.screen {
            screen.resize(grow)?;
        }
        Ok(())
    }
    pub fn builtin_hints(
        &mut self,
        definitions: impl Iterator<Item = (&'static str, &'static str)>,
    ) {
        if self.is_tui() {
            self.suggestions.builtin_hints(definitions);
        }
    }
    pub fn help_result(&mut self, output: Option<&str>) {
        if self.is_tui() {
            self.suggestions.help_result(output);
        }
    }
    pub fn move_suggestion(&mut self, down: bool) {
        self.suggestions.navigate(&self.draft, down);
    }
    pub fn accept_suggestion(&mut self) {
        if let Some(cursor) = self.suggestions.accept(&mut self.draft) {
            self.cursor = cursor;
        }
    }
    pub fn reset_suggestion(&mut self) {
        self.suggestions.reset();
    }
    pub fn history_divider(&mut self) -> Result<()> {
        if let Some(screen) = &mut self.screen {
            screen.divider()?;
        } else if !self.raw {
            inline::console_divider(&mut io::stdout().lock(), terminal_width())?;
        }
        Ok(())
    }
    pub fn logs(&mut self, source: Source, bytes: &[u8]) -> Result<()> {
        if self.raw {
            if source == Source::Stdout {
                let mut output = io::stdout().lock();
                output.write_all(bytes)?;
                output.flush()?;
            } else {
                let mut output = io::stderr().lock();
                for segment in bytes.split_inclusive(|byte| *byte == b'\n') {
                    if self.stderr_start {
                        output.write_all(b"[java stderr] ")?;
                    }
                    output.write_all(segment)?;
                    self.stderr_start = segment.ends_with(b"\n");
                }
                output.flush()?;
            }
            return Ok(());
        }
        let index = usize::from(source == Source::Stderr);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[index];
        self.streams[index].feed(bytes, |text, first, newline| {
            emit(screen, formatting, index, text, first, newline)
        })
    }
    pub fn finish_source(&mut self, source: Source) -> Result<()> {
        let index = usize::from(source == Source::Stderr);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[index];
        self.streams[index]
            .finish(|text, first, newline| emit(screen, formatting, index, text, first, newline))
    }
    pub fn diagnostic(&mut self, bytes: &[u8], dropped: usize) -> Result<()> {
        if dropped > 0 {
            self.notice(
                "diagnostics",
                &format!("{dropped} terminal log bytes omitted while output was busy"),
            )?;
        }
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[2];
        self.streams[2].feed(bytes, |text, first, newline| {
            emit(screen, formatting, 2, text, first, newline)
        })
    }
    pub fn notice(&mut self, source: &str, message: &str) -> Result<()> {
        let clean = text::clean(message);
        for line in clean.split('\n') {
            if let Some(screen) = &mut self.screen {
                screen.line(Line::from(vec![
                    Span::styled(format!("[{source}] "), style(Role::Muted)),
                    Span::raw(line),
                ]))?;
            } else {
                let mut output = io::stderr().lock();
                if !self.stderr_start {
                    writeln!(output)?;
                    self.stderr_start = true;
                }
                writeln!(output, "[{source}] {line}")?;
                output.flush()?;
            }
        }
        Ok(())
    }
    pub fn command(
        &mut self,
        command: &str,
        result: &CommandResult,
        other: bool,
        listener_id: &str,
    ) -> Result<()> {
        let name = text::clean(command.split_whitespace().next().unwrap_or_default());
        let origin = if other {
            format!("Executed by another Listener · {listener_id}")
        } else {
            String::new()
        };
        let label = if command == "@kill" {
            "Control"
        } else if result.channel == Channel::Rcon {
            "RCON"
        } else {
            "stdin"
        };
        let mut details = String::new();
        let output = if let Some(output) = &result.output {
            output.as_str()
        } else {
            details.push_str(match result.result {
                CommandOutcome::Sent if command == "@kill" => "Java force-kill requested",
                CommandOutcome::Sent => "sent; command output is in Java stdout",
                CommandOutcome::Completed => "completed",
                CommandOutcome::Unknown => "outcome unknown; command was not replayed",
            });
            if let Some(diagnostic) = &result.diagnostic {
                details.push('\n');
                details.push_str(diagnostic);
            }
            &details
        };
        if self.raw {
            let source = if other {
                format!("{label}: {name} · {origin}")
            } else {
                format!("{label}: {name}")
            };
            self.notice(&source, output)?;
        } else {
            let output = text::clean(output);
            // Render very large responses in bounded standard-widget pages, preserving one frame.
            let mut start = 0;
            loop {
                let mut end = (start + 8192).min(output.len());
                while !output.is_char_boundary(end) {
                    end -= 1;
                }
                let first = start == 0;
                let last = end == output.len();
                if let Some(screen) = &mut self.screen {
                    screen.rcon(label, &name, &origin, &output[start..end], first, last)?;
                } else {
                    inline::console_response(
                        &mut io::stderr().lock(),
                        label,
                        &name,
                        &origin,
                        &output[start..end],
                        terminal_width(),
                        terminal::stderr_color_enabled(),
                        first,
                        last,
                    )?;
                }
                if last {
                    break;
                }
                start = end;
            }
        }
        if result.output.is_some()
            && let Some(diagnostic) = &result.diagnostic
        {
            self.notice("command", diagnostic)?;
        }
        Ok(())
    }
    pub fn toggle_speed(&mut self, traffic: Option<SessionTrafficStats>) -> Result<()> {
        self.speed_full = !self.speed_full;
        if !self.is_tui() {
            for line in speed::lines(traffic, self.speed_full) {
                self.notice("speed", &line.to_string())?;
            }
        }
        Ok(())
    }
    pub fn draw(&mut self, traffic: Option<SessionTrafficStats>) -> Result<()> {
        let Some(screen) = &mut self.screen else {
            return Ok(());
        };
        let server_status = match self.server_status {
            ServerStatus::Unknown => "Unknown",
            ServerStatus::RconConnected => "RCON Connected",
        };
        let status = format!(
            "Java {:?} · Server {} · {:?} · @detach / Ctrl+D: detach · Ctrl+C: stop / kill",
            self.state, server_status, self.channel
        );
        let partial = self
            .streams
            .iter()
            .find(|stream| !stream.partial().is_empty())
            .map_or("", text::Stream::partial);
        screen.draw(
            &self.draft,
            self.cursor,
            &status,
            partial,
            self.suggestions.view(&self.draft),
            speed::lines(traffic, self.speed_full),
        )?;
        Ok(())
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        while let Ok((bytes, dropped)) = self.diagnostics.try_recv() {
            let _ = self.diagnostic(&bytes, dropped);
        }
        let _ = self.finish_source(Source::Stdout);
        let _ = self.finish_source(Source::Stderr);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[2];
        let _ = self.streams[2]
            .finish(|text, first, newline| emit(screen, formatting, 2, text, first, newline));
    }
}
fn terminal_width() -> u16 {
    crossterm::terminal::size().map_or(80, |(width, _)| width.max(3))
}
fn emit(
    screen: &mut Option<inline::Screen>,
    formatting: &mut log::Formatting,
    origin: usize,
    text: &str,
    first: bool,
    newline: bool,
) -> Result<()> {
    let fragment = formatting.fragment(text, first && origin < 2, newline);
    if let Some(screen) = screen {
        let mut line = log::line(text, &fragment);
        if origin == 1 && first {
            line.spans
                .insert(0, Span::styled("[java stderr] ", style(Role::Muted)));
        }
        screen.line(line)?;
    } else if origin == 0 {
        let mut output = io::stdout().lock();
        log::write(
            &mut output,
            text,
            &fragment,
            terminal::stdout_color_enabled(),
        )?;
        if newline {
            writeln!(output)?;
        }
        output.flush()?;
    } else {
        let mut output = io::stderr().lock();
        if origin == 1 && first {
            output.write_all(b"[java stderr] ")?;
        }
        log::write(
            &mut output,
            text,
            &fragment,
            terminal::stderr_color_enabled(),
        )?;
        if newline {
            writeln!(output)?;
        }
        output.flush()?;
    }
    Ok(())
}
