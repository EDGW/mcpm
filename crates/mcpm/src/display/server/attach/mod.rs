//! Inline/Console/Raw attach output; Minecraft prefix coloring is exclusively frontend presentation.

use crate::{logging, terminal, theme::widgets::style};
use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use mcpm_backend::operations::server::shim::{
    Channel, CommandOutcome, CommandResult, CrashReportType, ExitReason, LaunchExit, ServerStatus,
    Source, State,
};
use mcpm_exec::messaging::SessionTrafficStats;
use mcpm_rich::Role;
use ratatui::text::{Line, Span};
use std::{
    io::{self, Write},
    time::Instant,
};

mod command_hints;
mod inline;
mod log;
mod speed;
mod suggestions;
mod text;
pub(crate) use inline::modifier_name;
pub(crate) use suggestions::help_page_count;

struct ConsoleRestartBar {
    bar: ProgressBar,
    _logs: logging::ProgressGuard,
}

impl ConsoleRestartBar {
    fn new(delay_ms: u64) -> Self {
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(20));
        let bar = multi.add(ProgressBar::new(delay_ms.max(1)));
        bar.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template(
                "  [{wide_bar}] {msg}",
            ))
            .expect("restart progress template must be valid")
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        let logs = logging::attach_progress(Some(multi));
        Self { bar, _logs: logs }
    }
}

impl Drop for ConsoleRestartBar {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

struct RestartCountdown {
    attempt: u32,
    rule: String,
    delay_ms: u64,
    initial_remaining_ms: u64,
    anchored_at: Instant,
    console_bar: Option<ConsoleRestartBar>,
}

impl RestartCountdown {
    fn new(
        attempt: u32,
        rule: String,
        delay_ms: u64,
        remaining_ms: u64,
        console_bar: Option<ConsoleRestartBar>,
    ) -> Self {
        Self {
            attempt,
            rule,
            delay_ms,
            initial_remaining_ms: remaining_ms.min(delay_ms),
            anchored_at: Instant::now(),
            console_bar,
        }
    }

    fn remaining_ms(&self) -> u64 {
        let since_anchor = u64::try_from(self.anchored_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.initial_remaining_ms.saturating_sub(since_anchor)
    }

    fn elapsed_ms(&self) -> u64 {
        self.delay_ms.saturating_sub(self.remaining_ms())
    }

    fn ratio(&self) -> f64 {
        if self.delay_ms == 0 {
            1.0
        } else {
            (self.elapsed_ms() as f64 / self.delay_ms as f64).clamp(0.0, 1.0)
        }
    }

    fn remaining_secs(&self) -> u64 {
        self.remaining_ms().div_ceil(1_000)
    }

    fn countdown_label(&self) -> String {
        format!("Restarting in {}s", self.remaining_secs())
    }

    fn update_console_bar(&self) {
        if let Some(console) = &self.console_bar {
            let pos = if self.delay_ms == 0 {
                1
            } else {
                self.elapsed_ms().min(self.delay_ms)
            };
            console.bar.set_position(pos);
            console.bar.set_message(self.countdown_label());
        }
    }
}

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
    restart_countdown: Option<RestartCountdown>,
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
        traffic: Option<SessionTrafficStats>,
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
                Some(inline::Screen::new(height, speed::lines(traffic, false))?)
            } else {
                None
            },
            raw: !terminal::stdout_is_tty(),
            streams: Default::default(),
            formatting: Default::default(),
            stderr_start: true,
            draft: String::new(),
            cursor: 0,
            restart_countdown: None,
            state,
            channel,
            suggestions: suggestions::Suggestions::default(),
            speed_full: false,
            diagnostics,
            _console: guard,
        })
    }
    pub fn set_state(&mut self, state: State) {
        if !matches!(state, State::Restarting { .. }) {
            self.restart_countdown = None;
        }
        self.state = state;
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
    pub fn help_result(&mut self, pages: Option<&[String]>) {
        if self.is_tui() {
            self.suggestions.help_result(pages);
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
            self.suspend_console_bar(|| {
                inline::console_divider(&mut io::stdout().lock(), terminal_width())
            })?;
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
        let console_bar = self
            .restart_countdown
            .as_ref()
            .and_then(|countdown| countdown.console_bar.as_ref())
            .map(|console| &console.bar);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[index];
        let stderr_start = &mut self.stderr_start;
        let stream = &mut self.streams[index];
        let mut feed = || {
            stream.feed(bytes, |text, first, newline| {
                emit(screen, formatting, stderr_start, index, text, first, newline)
            })
        };
        if let Some(bar) = console_bar {
            bar.suspend(feed)
        } else {
            feed()
        }
    }
    pub fn finish_source(&mut self, source: Source) -> Result<()> {
        let index = usize::from(source == Source::Stderr);
        let console_bar = self
            .restart_countdown
            .as_ref()
            .and_then(|countdown| countdown.console_bar.as_ref())
            .map(|console| &console.bar);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[index];
        let stderr_start = &mut self.stderr_start;
        let stream = &mut self.streams[index];
        let mut finish = || {
            stream.finish(|text, first, newline| {
                emit(screen, formatting, stderr_start, index, text, first, newline)
            })
        };
        if let Some(bar) = console_bar {
            bar.suspend(finish)
        } else {
            finish()
        }
    }
    pub fn diagnostic(&mut self, bytes: &[u8], dropped: usize) -> Result<()> {
        if dropped > 0 {
            self.notice(
                "diagnostics",
                &format!("{dropped} terminal log bytes omitted while output was busy"),
            )?;
        }
        let console_bar = self
            .restart_countdown
            .as_ref()
            .and_then(|countdown| countdown.console_bar.as_ref())
            .map(|console| &console.bar);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[2];
        let stderr_start = &mut self.stderr_start;
        let stream = &mut self.streams[2];
        let mut feed = || {
            stream.feed(bytes, |text, first, newline| {
                emit(screen, formatting, stderr_start, 2, text, first, newline)
            })
        };
        if let Some(bar) = console_bar {
            bar.suspend(feed)
        } else {
            feed()
        }
    }
    fn suspend_console_bar<R>(&self, action: impl FnOnce() -> R) -> R {
        if let Some(console) = self
            .restart_countdown
            .as_ref()
            .and_then(|countdown| countdown.console_bar.as_ref())
        {
            console.bar.suspend(action)
        } else {
            action()
        }
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
                let mut stderr_start = self.stderr_start;
                self.suspend_console_bar(|| -> io::Result<()> {
                    let mut output = io::stderr().lock();
                    if !stderr_start {
                        writeln!(output)?;
                        stderr_start = true;
                    }
                    writeln!(output, "[{source}] {line}")?;
                    output.flush()
                })?;
                self.stderr_start = stderr_start;
            }
        }
        Ok(())
    }
    pub fn restarting(
        &mut self,
        rule: &str,
        attempt: u32,
        delay_ms: u64,
        remaining_ms: u64,
    ) -> Result<()> {
        let clean_rule = sanitize_field(rule);
        if self.raw {
            let remaining = format_delay_ms(remaining_ms);
            return self.notice(
                "restart",
                &format!(
                    "Restarting in {remaining} (rule: {clean_rule}, attempt #{attempt}); @stop / @kill / Ctrl+C to cancel"
                ),
            );
        }
        if self.screen.is_some() {
            // Inline TUI renders the live Primary-bordered Restart card + smooth Gauge in the viewport.
            self.restart_countdown = Some(RestartCountdown::new(
                attempt,
                clean_rule,
                delay_ms,
                remaining_ms,
                None,
            ));
            return Ok(());
        }
        // Console mode (--no-ui): print the Primary-bordered card, then drive a smooth stderr bar.
        self.restart_countdown = None;
        let lines = restart_info_lines(&clean_rule, delay_ms, true);
        {
            let mut output = io::stderr().lock();
            if !self.stderr_start {
                writeln!(output)?;
                self.stderr_start = true;
            }
            inline::console_restart_report(
                &mut output,
                attempt,
                lines,
                terminal_width(),
                terminal::stderr_color_enabled(),
            )?;
            output.flush()?;
        }
        let bar = terminal::stderr_is_tty().then(|| ConsoleRestartBar::new(delay_ms));
        let countdown = RestartCountdown::new(attempt, clean_rule, delay_ms, remaining_ms, bar);
        countdown.update_console_bar();
        self.restart_countdown = Some(countdown);
        Ok(())
    }
    pub fn restart_started(&mut self, rule: &str, attempt: u32, delay_ms: u64) -> Result<()> {
        self.restart_countdown = None;
        if let Some(screen) = &mut self.screen {
            let clean_rule = sanitize_field(rule);
            let lines = restart_info_lines(&clean_rule, delay_ms, false);
            screen.restart_report(attempt, lines)?;
        }
        Ok(())
    }
    pub fn exited(&mut self, exit: &ExitReason) -> Result<()> {
        self.restart_countdown = None;
        if self.raw {
            let label = format_exit(exit);
            let mut output = io::stderr().lock();
            if !self.stderr_start {
                writeln!(output)?;
                self.stderr_start = true;
            }
            writeln!(output, "[exit] Java terminated: {label}")?;
            output.flush()?;
            return Ok(());
        }
        let (title, role, lines) = exit_report(exit);
        if let Some(screen) = &mut self.screen {
            screen.exit_report(title, role, lines)?;
        } else {
            let mut output = io::stderr().lock();
            if !self.stderr_start {
                writeln!(output)?;
                self.stderr_start = true;
            }
            inline::console_exit_report(
                &mut output,
                title,
                role,
                lines,
                terminal_width(),
                terminal::stderr_color_enabled(),
            )?;
            output.flush()?;
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
                    self.suspend_console_bar(|| {
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
                        )
                    })?;
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
            if let Some(countdown) = &self.restart_countdown {
                countdown.update_console_bar();
            }
            return Ok(());
        };
        let restart_view = self.restart_countdown.as_ref().map(|countdown| {
            inline::RestartView {
                attempt: countdown.attempt,
                lines: restart_info_lines(&countdown.rule, countdown.delay_ms, true),
                ratio: countdown.ratio(),
                countdown_label: countdown.countdown_label(),
            }
        });
        let status = match &self.state {
            State::Restarting { attempt, .. } => format!(
                "Java Restarting · attempt #{attempt} · @stop / @kill / Ctrl+C: cancel · @detach / Ctrl+D: detach"
            ),
            _ => {
                let server_status = match self.state.server_status() {
                    ServerStatus::Unknown => "Unknown",
                    ServerStatus::RconConnected => "RCON Connected",
                };
                format!(
                    "Java {} · Server {} · {:?} · @detach / Ctrl+D: detach · Ctrl+C: stop / kill",
                    self.state.label(),
                    server_status,
                    self.channel
                )
            }
        };
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
            restart_view,
            self.suggestions.view(&self.draft),
            speed::lines(traffic, self.speed_full),
        )?;
        Ok(())
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        self.restart_countdown = None;
        while let Ok((bytes, dropped)) = self.diagnostics.try_recv() {
            let _ = self.diagnostic(&bytes, dropped);
        }
        let _ = self.finish_source(Source::Stdout);
        let _ = self.finish_source(Source::Stderr);
        let screen = &mut self.screen;
        let formatting = &mut self.formatting[2];
        let stderr_start = &mut self.stderr_start;
        let _ = self.streams[2].finish(|text, first, newline| {
            emit(screen, formatting, stderr_start, 2, text, first, newline)
        });
    }
}
/// Formats a classified Java termination reason for attach diagnostics and raw exit notices.
pub(crate) fn format_exit(exit: &ExitReason) -> String {
    match exit {
        ExitReason::KilledByUser => "KilledByUser".into(),
        ExitReason::KilledByMcpm {
            exit: LaunchExit::Signal(ExitReason::SIGKILL),
        } => "KilledByMcpm(SIGKILL)".into(),
        ExitReason::KilledByMcpm { .. } => "KilledByMcpm".into(),
        ExitReason::Exited => "EXITED(0)".into(),
        ExitReason::Crashed {
            code,
            crash_report: Some(report),
        } => {
            let path = sanitize_field(&report.path);
            match &report.analysis {
                Ok(analysis) if analysis.report_type == CrashReportType::Watchdog => {
                    format!("CRASHED({code}, watchdog, {path})")
                }
                Ok(_) | Err(_) => format!("CRASHED({code}, {path})"),
            }
        }
        ExitReason::Crashed {
            code,
            crash_report: None,
        } => format!("CRASHED({code})"),
    }
}

fn sanitize_field(value: &str) -> String {
    text::clean(value)
        .chars()
        .filter(|ch| !ch.is_control())
        .collect()
}

fn restart_info_lines(rule: &str, delay_ms: u64, waiting: bool) -> Vec<Line<'static>> {
    let field = |label: &'static str, value: String| {
        Line::from(vec![
            Span::styled(format!("{label}: "), style(Role::Muted)),
            Span::raw(value),
        ])
    };
    let status = if waiting {
        "Waiting before respawning Java (@stop / @kill / Ctrl+C to cancel)"
    } else {
        "Respawning Java"
    };
    vec![
        field("Status", status.into()),
        field("Rule", rule.to_owned()),
        field("Delay", format_delay_ms(delay_ms)),
    ]
}

fn format_delay_ms(delay_ms: u64) -> String {
    if delay_ms == 0 {
        return "0s".into();
    }
    if delay_ms < 1_000 {
        return format!("{delay_ms}ms");
    }
    let secs = delay_ms / 1_000;
    let rem_ms = delay_ms % 1_000;
    if rem_ms == 0 {
        format_seconds(secs)
    } else {
        format!("{secs}.{:03}s", rem_ms)
    }
}

fn format_seconds(total_secs: u64) -> String {
    if total_secs < 60 {
        return format!("{total_secs}s");
    }
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins < 60 {
        return if secs == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m{secs}s")
        };
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if rem_mins == 0 && secs == 0 {
        format!("{hours}h")
    } else if secs == 0 {
        format!("{hours}h{rem_mins}m")
    } else {
        format!("{hours}h{rem_mins}m{secs}s")
    }
}

fn exit_report(exit: &ExitReason) -> (&'static str, Role, Vec<Line<'static>>) {
    let field = |label: &'static str, value: String| {
        Line::from(vec![
            Span::styled(format!("{label}: "), style(Role::Muted)),
            Span::raw(value),
        ])
    };
    match exit {
        ExitReason::Exited => (
            "Exited",
            Role::Success,
            vec![
                field("Status", "Java exited cleanly".into()),
                field("Exit code", "0".into()),
            ],
        ),
        ExitReason::KilledByMcpm { exit: launch_exit } => {
            let mut lines = vec![field("Status", "Java was force-killed by mcpm".into())];
            match launch_exit {
                LaunchExit::Signal(ExitReason::SIGKILL) => {
                    lines.push(field(
                        "Signal",
                        format!(
                            "SIGKILL ({}) · Exit code {}",
                            ExitReason::SIGKILL,
                            exit.shell_code()
                        ),
                    ));
                }
                LaunchExit::Signal(signal) => {
                    lines.push(field(
                        "Signal",
                        format!("{signal} · Exit code {}", exit.shell_code()),
                    ));
                }
                LaunchExit::Code(code) => {
                    lines.push(field("Exit code", code.to_string()));
                }
            }
            ("Killed by mcpm", Role::Warning, lines)
        }
        ExitReason::KilledByUser => (
            "Killed by user",
            Role::Warning,
            vec![
                field("Status", "Java was terminated externally (SIGKILL)".into()),
                field("Exit code", exit.shell_code().to_string()),
            ],
        ),
        ExitReason::Crashed { code, crash_report } => {
            let mut title = "Crashed";
            let mut lines = vec![
                field("Status", "Java crashed unexpectedly".into()),
                field("Exit code", code.to_string()),
            ];
            if let Some(report) = crash_report {
                match &report.analysis {
                    Ok(analysis) => {
                        let crash_type = match analysis.report_type {
                            CrashReportType::Watchdog => {
                                title = "Crashed (Watchdog)";
                                "Watchdog"
                            }
                            CrashReportType::Others => "Others",
                            CrashReportType::Unknown => "Unknown",
                        };
                        lines.push(field("Crash type", crash_type.into()));
                        if let Some(desc) = &analysis.desc {
                            lines.push(field("Description", sanitize_field(desc)));
                        }
                        lines.push(field("Crash report", sanitize_field(&report.path)));
                    }
                    Err(error) => {
                        lines.push(field("Crash report", sanitize_field(&report.path)));
                        lines.push(Line::from(Span::styled(
                            "Crash report analysis:",
                            style(Role::Muted),
                        )));
                        for line in crate::display::error::lines(error, false) {
                            lines.push(Line::from(Span::raw(format!(
                                "  {}",
                                sanitize_field(&line)
                            ))));
                        }
                    }
                }
            }
            (title, Role::Danger, lines)
        }
    }
}

fn terminal_width() -> u16 {
    crossterm::terminal::size().map_or(80, |(width, _)| width.max(3))
}
fn emit(
    screen: &mut Option<inline::Screen>,
    formatting: &mut log::Formatting,
    stderr_start: &mut bool,
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
        *stderr_start = newline;
        output.flush()?;
    }
    Ok(())
}
