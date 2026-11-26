//! Numbered Console launch selection using the controlling terminal.

use std::{io, path::Path};

use anyhow::{Context, Result};
use console::{Key, Term};
#[cfg(windows)]
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use mcpm_backend::operations::server::init::Candidate;

use super::presentation;
use crate::display::style::OutputStyle;

pub(super) fn select(
    candidates: &[Candidate],
    command: Option<&str>,
    invoked_path: &Path,
    replace_conf: bool,
) -> Result<Option<usize>> {
    // stdout controls interaction; input comes from the terminal, never redirected stdin.
    let term = Term::stdout();
    let style = OutputStyle::new();
    term.write_line("Select an identified server launch to initialize:")?;
    let label = if cfg!(windows) {
        "Manual init (PowerShell): "
    } else {
        "Manual init: "
    };
    let invocation = command.zip(invoked_path.to_str());
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
        let manual = invocation
            .and_then(|(prefix, path)| {
                presentation::manual_command(prefix, path, candidate, replace_conf)
            })
            .map_or_else(
                || style.muted("(cannot represent this candidate as a single-line command)"),
                |command| style.code(command),
            );
        term.write_line(&format!("{}{}", style.normal(label), manual))?;
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
