//! Local Console and TUI launch selection; RAII restores terminal state on every exit.

use std::io::{self, Write};

use anyhow::{Context, Result, ensure};
use console::{Key, Term};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use mcpm_backend::operations::server::init::Candidate;
use mcpm_rich::Role;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use super::presentation;
use crate::{
    display::style::OutputStyle,
    terminal as policy,
    theme::widgets::{loader, style},
};

pub(super) fn select(candidates: &[Candidate], no_ui: bool) -> Result<Option<usize>> {
    ensure!(
        policy::stdout_is_tty(),
        "server launch selection requires terminal stdout"
    );
    if no_ui || !policy::stderr_is_tty() {
        return console(candidates);
    }
    let summaries: Vec<_> = candidates.iter().map(presentation::summary).collect();
    let mut screen = Screen {
        terminal: Terminal::new(CrosstermBackend::new(io::stderr()))
            .context("failed to open server selector")?,
        raw: false,
        alternate: false,
    };
    terminal::enable_raw_mode().context("failed to enable server selector")?;
    screen.raw = true;
    screen.alternate = true;
    execute!(screen.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
    let mut selected = 0;
    loop {
        screen.render(candidates, &summaries, selected)?;
        if let Event::Key(key) = event::read()? {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match key.code {
                KeyCode::Up => selected = (selected + candidates.len() - 1) % candidates.len(),
                KeyCode::Down => selected = (selected + 1) % candidates.len(),
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = candidates.len() - 1,
                KeyCode::Enter => return Ok(Some(selected)),
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c' | 'd' | 'z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
}

fn console(candidates: &[Candidate]) -> Result<Option<usize>> {
    // stdout controls interaction; input comes from the terminal, never redirected stdin.
    let term = Term::stdout();
    let style = OutputStyle::new();
    term.write_line("Select an identified server launch to initialize:")?;
    for (index, candidate) in candidates.iter().enumerate() {
        let summary = presentation::summary(candidate);
        term.write_line(&format!(
            "{}. {}",
            index + 1,
            style.primary(&summary.identity)
        ))?;
        for detail in summary.details() {
            let line: String = detail
                .into_iter()
                .map(|(role, value)| style.role(role, value))
                .collect();
            term.write_line(&line)?;
        }
        term.write_line("")?;
    }
    loop {
        term.write_str(&format!(
            "Select 1-{} (Enter/Esc/EOF cancels): ",
            candidates.len()
        ))?;
        let Some(answer) = read_number(&term).context("failed to read server launch selection")?
        else {
            return Ok(None);
        };
        if let Ok(index) = answer.parse::<usize>()
            && (1..=candidates.len()).contains(&index)
        {
            return Ok(Some(index - 1));
        }
        term.write_line(
            &style.warning("Please enter a candidate number, or press Enter to cancel."),
        )?;
    }
}

fn read_number(term: &Term) -> io::Result<Option<String>> {
    let mut answer = String::new();
    #[cfg(unix)]
    let mut input = ConsoleInput::open()?;
    loop {
        #[cfg(unix)]
        let key = input.read_key();
        #[cfg(windows)]
        let key = read_key(term);
        let key = match key {
            Ok(key) => key,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::Interrupted
                ) =>
            {
                term.write_line("")?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        match key {
            Key::Enter => {
                term.write_line("")?;
                return Ok((!answer.is_empty()).then_some(answer));
            }
            Key::Escape | Key::CtrlC | Key::Char('\u{3}' | '\u{4}' | '\u{1a}') => {
                term.write_line("")?;
                return Ok(None);
            }
            Key::Char(ch) if ch.is_ascii_digit() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            Key::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}

/// Read only /dev/tty, even when stdin points at another terminal or a pipe.
#[cfg(unix)]
struct ConsoleInput {
    input: std::fs::File,
    original: libc::termios,
}
#[cfg(unix)]
impl ConsoleInput {
    fn open() -> io::Result<Self> {
        use std::os::fd::AsRawFd;
        let input = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")?;
        let mut original = std::mem::MaybeUninit::uninit();
        // SAFETY: input is a live terminal descriptor; tcgetattr initializes on success.
        if unsafe { libc::tcgetattr(input.as_raw_fd(), original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let original = unsafe { original.assume_init() };
        let mut mode = original;
        mode.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        mode.c_cc[libc::VMIN] = 1;
        mode.c_cc[libc::VTIME] = 0;
        // SAFETY: mode is initialized and the descriptor remains owned by input.
        if unsafe { libc::tcsetattr(input.as_raw_fd(), libc::TCSANOW, &mode) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { input, original })
    }

    fn read_key(&mut self) -> io::Result<Key> {
        use std::io::Read;
        let mut byte = [0];
        if self.input.read(&mut byte)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(match byte[0] {
            b'\r' | b'\n' => Key::Enter,
            0x1b => Key::Escape,
            0x03 => Key::CtrlC,
            0x08 | 0x7f => Key::Backspace,
            value if value.is_ascii() => Key::Char(char::from(value)),
            _ => Key::Unknown,
        })
    }
}
#[cfg(unix)]
impl Drop for ConsoleInput {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is still owned and original came from tcgetattr.
        unsafe {
            libc::tcsetattr(self.input.as_raw_fd(), libc::TCSANOW, &self.original);
        }
    }
}

/// Use CONIN$ on Windows, even if stdin was redirected. Restore raw mode before echoing.
#[cfg(windows)]
fn read_key(_term: &Term) -> io::Result<Key> {
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    terminal::enable_raw_mode()?;
    let _raw = Raw;
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        return Ok(match key.code {
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Escape,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Char('c' | 'd' | 'z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Key::Escape
            }
            KeyCode::Char(ch) => Key::Char(ch),
            _ => Key::Unknown,
        });
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
    raw: bool,
    alternate: bool,
}
impl Screen {
    fn render(
        &mut self,
        candidates: &[Candidate],
        summaries: &[presentation::Summary],
        selected: usize,
    ) -> io::Result<()> {
        self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                Paragraph::new("Select an identified server launch to initialize")
                    .style(style(Role::Primary)),
                Rect::new(0, 0, area.width, area.height.min(1)),
            );
            let available = area.height.saturating_sub(2);
            let card_height = available.clamp(1, 6);
            let visible = usize::from((available / card_height).max(1));
            let start = selected
                .saturating_sub(visible / 2)
                .min(candidates.len().saturating_sub(visible));
            for (offset, index) in (start..candidates.len()).take(visible).enumerate() {
                let candidate = &candidates[index];
                let summary = &summaries[index];
                let loader_id = candidate
                    .metadata
                    .loader
                    .declaration()
                    .map_or("vanilla", |loader| loader.loader_type.as_str());
                let title = Line::styled(
                    format!(" {}. {} ", index + 1, summary.identity),
                    loader(loader_id),
                );
                let card = Block::bordered()
                    .title(title)
                    .padding(Padding::horizontal(2))
                    .border_style(style(if index == selected {
                        Role::Primary
                    } else {
                        Role::Muted
                    }));
                let lines: Vec<_> = summary
                    .details()
                    .into_iter()
                    .map(|detail| {
                        Line::from(
                            detail
                                .into_iter()
                                .map(|(role, value)| Span::styled(value, style(role)))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                let rect = Rect::new(
                    0,
                    1 + offset as u16 * card_height,
                    area.width,
                    card_height.min(area.height.saturating_sub(1 + offset as u16 * card_height)),
                );
                frame.render_widget(
                    Paragraph::new(lines).style(style(Role::Normal)).block(card),
                    rect,
                );
            }
            frame.render_widget(
                Paragraph::new("Up/Down: move | Enter: initialize | Esc/Ctrl-C: cancel"),
                Rect::new(
                    0,
                    area.height.saturating_sub(1),
                    area.width,
                    area.height.min(1),
                ),
            );
        })?;
        Ok(())
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
        let _ = self.terminal.backend_mut().flush();
    }
}
